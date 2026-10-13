//! KRP's text: `renderShadedAnimText` bakes a string into a canvas once,
//! as a stack of darker copies under the coloured top layer, and the game
//! draws that canvas scaled. Same here, with the game's own font.

#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::rc::Rc;

use macroquad::prelude::*;

use crate::gfx::{Canvas, Image, Materials, Painter, hex, shade};

/// `font2.ttf`'s ascent and descent (hhea), in em. Chrome puts a
/// `textBaseline = "middle"` string's baseline half their difference below
/// the given y.
const ASCENT: f32 = 1.125;
const DESCENT: f32 = 0.25;

/// How far the browser leans its synthetic italic (Skia's fake italic).
const ITALIC_SHEAR: f32 = 0.25;

/// More than this many baked strings and the oldest half is dropped.
const MAX_CACHED: usize = 600;

pub struct Text {
    pub font: Font,
    mats: Rc<Materials>,
    cache: HashMap<String, (Canvas, Option<Canvas>, u64)>,
    tick: u64,
}

/// A baked string and the size KRP's canvas for it has.
pub struct Baked {
    pub image: Image,
    pub width: f32,
    pub height: f32,
}

impl Text {
    #[must_use]
    pub fn new(font: Font, mats: Rc<Materials>) -> Self {
        Self {
            font,
            mats,
            cache: HashMap::new(),
            tick: 0,
        }
    }

    /// `ctx.measureText(text).width` at `size` px.
    #[must_use]
    pub fn measure(&self, text: &str, size: f32) -> f32 {
        let (fs, scale) = font_size(size);
        measure_text(text, Some(&self.font), fs, scale).width
    }

    /// Width of `text` at `size` CSS px, rasterised at `px` device pixels
    /// per CSS pixel (the HUD and menus).
    #[must_use]
    pub fn measure_px(&self, text: &str, size: f32, px: f32) -> f32 {
        let (fs, scale) = font_size(size * px);
        measure_text(text, Some(&self.font), fs, scale / px).width
    }

    /// [`Text::draw`] for the HUD and menus: `size` is in CSS px and the
    /// glyphs are rasterised at `px` device pixels per CSS pixel, so they
    /// stay sharp on high-density screens.
    pub fn draw_px(&self, text: &str, x: f32, y: f32, size: f32, px: f32, color: Color) {
        let (fs, scale) = font_size(size * px);
        draw_text_ex(
            text,
            x,
            y,
            TextParams {
                font: Some(&self.font),
                font_size: fs,
                font_scale: scale / px,
                color,
                ..Default::default()
            },
        );
    }

    /// KRP `renderShadedAnimText(text, fontSize, color, layerCount,
    /// fontExtra)`. Bakes on first use, so the caller must restore its own
    /// camera afterwards (see [`Text::render_shaded`]'s `restore`).
    pub fn render_shaded(
        &mut self,
        text: &str,
        size: f32,
        color: &str,
        layers: u32,
        italic: bool,
        restore: &mut dyn FnMut(),
    ) -> Baked {
        self.tick += 1;
        let key = format!("{text}{size}{color}{layers}{italic}");
        if let Some((c, _, used)) = self.cache.get_mut(&key) {
            *used = self.tick;
            return Baked {
                image: c.image(),
                width: c.width,
                height: c.height,
            };
        }
        if self.cache.len() > MAX_CACHED {
            let mut ages: Vec<u64> = self.cache.values().map(|(_, _, u)| *u).collect();
            ages.sort_unstable();
            let cut = ages[ages.len() / 2];
            self.cache.retain(|_, (_, _, u)| *u > cut);
        }
        // Canvas sizes are integers; the browser truncates.
        let width = (self.measure(text, size) * 1.08).floor().max(1.0);
        let height = (size * 1.8 + layers as f32).floor().max(1.0);
        let canvas = Canvas::new(width as u32, height as u32);
        let mut p = Painter::new(Rc::clone(&self.mats));
        let center_x = width / 2.0;
        let center_y = height / 2.0;
        let text_w = self.measure(text, size);
        let base = center_y + size * (ASCENT - DESCENT) / 2.0;
        let x0 = center_x - text_w / 2.0;
        let draw_layers = |p: &mut Painter, font: &Font| {
            p.flush_premul_text();
            let (fs, scale) = font_size(size);
            let low = shade(color, -18.0);
            for i in 1..layers {
                draw_text_ex(
                    text,
                    x0,
                    base + i as f32,
                    TextParams {
                        font: Some(font),
                        font_size: fs,
                        font_scale: scale,
                        color: low,
                        ..Default::default()
                    },
                );
            }
            draw_text_ex(
                text,
                x0,
                base,
                TextParams {
                    font: Some(font),
                    font_size: fs,
                    font_scale: scale,
                    color: hex(color),
                    ..Default::default()
                },
            );
        };
        // The upright copy is kept with the result: macroquad draws at the
        // end of the frame, so it must outlive this call.
        let upright = if italic {
            let upright = Canvas::new(width as u32, height as u32);
            upright.begin(&mut p);
            draw_layers(&mut p, &self.font);
            canvas.begin(&mut p);
            p.draw_image_sheared(&upright.image(), 0.0, 0.0, width, height, ITALIC_SHEAR);
            Some(upright)
        } else {
            canvas.begin(&mut p);
            draw_layers(&mut p, &self.font);
            None
        };
        gl_use_default_material();
        restore();
        let baked = Baked {
            image: canvas.image(),
            width,
            height,
        };
        self.cache.insert(key, (canvas, upright, self.tick));
        baked
    }
}

/// macroquad sizes fonts in whole pixels; the rest goes in the scale.
fn font_size(size: f32) -> (u16, f32) {
    let fs = size.round().clamp(1.0, 400.0);
    (fs as u16, size / fs)
}
