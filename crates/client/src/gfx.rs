//! A small canvas-2D-like layer over macroquad, so the drawing code can
//! follow KRP's `CanvasRenderingContext2D` calls one for one.
//!
//! KRP draws with `translate`/`rotate`/`save`/`restore`, `globalAlpha`,
//! two composite modes (`source-over` and `lighter`) and offscreen canvases
//! (players, walls, floors, text). Offscreen canvases become render targets
//! here. They hold premultiplied colour, like a browser canvas does, so that
//! half-transparent parts composite the same way when drawn back.
//!
//! Render target textures come out upside down in macroquad, so drawing
//! one flips it back (`Image::flip_y`).

#![forbid(unsafe_code)]

use std::rc::Rc;

use macroquad::miniquad::{BlendFactor, BlendState, BlendValue, Equation, PipelineParams};
use macroquad::prelude::*;

/// A drawable image: a sprite from the asset pack, or a render target.
#[derive(Clone)]
pub struct Image {
    pub tex: Texture2D,
    /// Holds premultiplied colour (a render target).
    pub premul: bool,
    /// Drawn mirrored left to right (KRP's flipped "right" sprites).
    pub flip_x: bool,
    /// Drawn upside down (render targets, see the module notes).
    pub flip_y: bool,
}

impl Image {
    #[must_use]
    pub fn sprite(tex: Texture2D) -> Self {
        Self {
            tex,
            premul: false,
            flip_x: false,
            flip_y: false,
        }
    }

    #[must_use]
    pub fn flipped(&self) -> Self {
        Self {
            flip_x: !self.flip_x,
            ..self.clone()
        }
    }

    #[must_use]
    pub fn width(&self) -> f32 {
        self.tex.width()
    }

    #[must_use]
    pub fn height(&self) -> f32 {
        self.tex.height()
    }
}

/// An offscreen canvas.
pub struct Canvas {
    pub rt: RenderTarget,
    pub width: f32,
    pub height: f32,
}

impl Canvas {
    #[must_use]
    pub fn new(width: u32, height: u32) -> Self {
        let rt = render_target(width.max(1), height.max(1));
        rt.texture.set_filter(FilterMode::Nearest);
        Self {
            rt,
            width: width as f32,
            height: height as f32,
        }
    }

    #[must_use]
    pub fn image(&self) -> Image {
        Image {
            tex: self.rt.texture.clone(),
            premul: true,
            flip_x: false,
            flip_y: true,
        }
    }

    /// Makes this canvas the drawing target, cleared, with canvas pixel
    /// coordinates (origin top left).
    pub fn begin(&self, painter: &mut Painter) {
        set_camera(&Camera2D {
            render_target: Some(self.rt.clone()),
            ..Camera2D::from_display_rect(Rect::new(0.0, 0.0, self.width, self.height))
        });
        clear_background(Color::new(0.0, 0.0, 0.0, 0.0));
        painter.reset(true);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Blend {
    /// `source-over`
    Normal,
    /// `lighter`
    Additive,
    /// `source-atop` (only on offscreen canvases)
    Atop,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mat {
    /// Unknown: something else changed the material.
    Unknown,
    Default,
    Additive,
    PremulIn,
    PremulAtop,
    PremulAlpha,
    PremulTint,
}

const VERTEX: &str = r"#version 100
attribute vec3 position;
attribute vec2 texcoord;
attribute vec4 color0;
varying mediump vec2 uv;
varying mediump vec4 color;
uniform mat4 Model;
uniform mat4 Projection;
void main() {
    gl_Position = Projection * Model * vec4(position, 1);
    color = color0 / 255.0;
    uv = texcoord;
}
";

const FRAG_STRAIGHT: &str = r"#version 100
precision mediump float;
varying mediump vec2 uv;
varying mediump vec4 color;
uniform sampler2D Texture;
void main() {
    gl_FragColor = texture2D(Texture, uv) * color;
}
";

const FRAG_PREMUL_IN: &str = r"#version 100
precision mediump float;
varying mediump vec2 uv;
varying mediump vec4 color;
uniform sampler2D Texture;
void main() {
    vec4 c = texture2D(Texture, uv) * color;
    gl_FragColor = vec4(c.rgb * c.a, c.a);
}
";

const FRAG_PREMUL_ALPHA: &str = r"#version 100
precision mediump float;
varying mediump vec2 uv;
varying mediump vec4 color;
uniform sampler2D Texture;
void main() {
    gl_FragColor = texture2D(Texture, uv) * color.a;
}
";

// `source-atop` fill over a premultiplied canvas: colour `color.rgb` with
// strength `color.a`, only where the canvas has coverage.
const FRAG_PREMUL_TINT: &str = r"#version 100
precision mediump float;
varying mediump vec2 uv;
varying mediump vec4 color;
uniform sampler2D Texture;
void main() {
    vec4 t = texture2D(Texture, uv);
    gl_FragColor = vec4(t.rgb * (1.0 - color.a) + color.rgb * color.a * t.a, t.a);
}
";

fn material(frag: &str, color: BlendState, alpha: BlendState) -> Material {
    load_material(
        ShaderSource::Glsl {
            vertex: VERTEX,
            fragment: frag,
        },
        MaterialParams {
            pipeline_params: PipelineParams {
                color_blend: Some(color),
                alpha_blend: Some(alpha),
                ..Default::default()
            },
            ..Default::default()
        },
    )
    .expect("built-in shaders compile")
}

/// The materials for each blend and target combination.
pub struct Materials {
    additive: Material,
    premul_in: Material,
    premul_atop: Material,
    premul_alpha: Material,
    premul_tint: Material,
}

impl Materials {
    #[must_use]
    pub fn new() -> Self {
        let over = BlendState::new(
            Equation::Add,
            BlendFactor::One,
            BlendFactor::OneMinusValue(BlendValue::SourceAlpha),
        );
        let add = BlendState::new(
            Equation::Add,
            BlendFactor::Value(BlendValue::SourceAlpha),
            BlendFactor::One,
        );
        let keep = BlendState::new(Equation::Add, BlendFactor::Zero, BlendFactor::One);
        let atop = BlendState::new(
            Equation::Add,
            BlendFactor::Value(BlendValue::DestinationAlpha),
            BlendFactor::OneMinusValue(BlendValue::SourceAlpha),
        );
        Self {
            additive: material(FRAG_STRAIGHT, add, keep),
            premul_in: material(FRAG_PREMUL_IN, over, over),
            premul_atop: material(FRAG_PREMUL_IN, atop, keep),
            premul_alpha: material(FRAG_PREMUL_ALPHA, over, over),
            premul_tint: material(FRAG_PREMUL_TINT, over, over),
        }
    }
}

impl Default for Materials {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy)]
struct State {
    m: Affine2,
    alpha: f32,
    blend: Blend,
}

/// The drawing state of one context: transform, alpha, blend, target.
pub struct Painter {
    mats: Rc<Materials>,
    state: State,
    stack: Vec<State>,
    /// Drawing into an offscreen (premultiplied) canvas.
    offscreen: bool,
    current: Mat,
}

impl Painter {
    #[must_use]
    pub fn new(mats: Rc<Materials>) -> Self {
        Self {
            mats,
            state: State {
                m: Affine2::IDENTITY,
                alpha: 1.0,
                blend: Blend::Normal,
            },
            stack: Vec::new(),
            offscreen: false,
            current: Mat::Default,
        }
    }

    /// Fresh state for a new target.
    pub fn reset(&mut self, offscreen: bool) {
        self.state = State {
            m: Affine2::IDENTITY,
            alpha: 1.0,
            blend: Blend::Normal,
        };
        self.stack.clear();
        self.offscreen = offscreen;
        self.current = Mat::Default;
        gl_use_default_material();
    }

    pub fn save(&mut self) {
        self.stack.push(self.state);
    }

    pub fn restore(&mut self) {
        if let Some(s) = self.stack.pop() {
            self.state = s;
        }
    }

    pub fn translate(&mut self, x: f32, y: f32) {
        self.state.m *= Affine2::from_translation(vec2(x, y));
    }

    pub fn rotate(&mut self, angle: f32) {
        self.state.m *= Affine2::from_angle(angle);
    }

    pub fn set_alpha(&mut self, a: f32) {
        self.state.alpha = a.clamp(0.0, 1.0);
    }

    pub fn set_blend(&mut self, b: Blend) {
        self.state.blend = b;
    }

    fn use_mat(&mut self, m: Mat) {
        if m == self.current {
            return;
        }
        self.current = m;
        match m {
            Mat::Unknown | Mat::Default => gl_use_default_material(),
            Mat::Additive => gl_use_material(&self.mats.additive),
            Mat::PremulIn => gl_use_material(&self.mats.premul_in),
            Mat::PremulAtop => gl_use_material(&self.mats.premul_atop),
            Mat::PremulAlpha => gl_use_material(&self.mats.premul_alpha),
            Mat::PremulTint => gl_use_material(&self.mats.premul_tint),
        }
    }

    fn mat_for(&self, premul_src: bool) -> Mat {
        match (premul_src, self.state.blend, self.offscreen) {
            (true, _, _) => Mat::PremulAlpha,
            (false, Blend::Additive, _) => Mat::Additive,
            (false, Blend::Atop, true) => Mat::PremulAtop,
            (false, Blend::Normal | Blend::Atop, false) => Mat::Default,
            (false, Blend::Normal, true) => Mat::PremulIn,
        }
    }

    fn corner(&self, x: f32, y: f32) -> (Vec2, f32) {
        let p = self.state.m.transform_point2(vec2(x, y));
        let angle = self
            .state
            .m
            .matrix2
            .x_axis
            .y
            .atan2(self.state.m.matrix2.x_axis.x);
        (p, angle)
    }

    /// `drawImage(img, dx, dy, dw, dh)`.
    pub fn draw_image(&mut self, img: &Image, dx: f32, dy: f32, dw: f32, dh: f32) {
        self.draw_image_colored(img, dx, dy, dw, dh, Color::new(1.0, 1.0, 1.0, 1.0), false);
    }

    /// `drawImage` with a colour multiplied in; `tint` draws a premultiplied
    /// image with a `source-atop` colour fill (`color.a` is its strength).
    #[allow(clippy::too_many_arguments)]
    pub fn draw_image_colored(
        &mut self,
        img: &Image,
        dx: f32,
        dy: f32,
        dw: f32,
        dh: f32,
        color: Color,
        tint: bool,
    ) {
        if dw == 0.0 || dh == 0.0 {
            return;
        }
        let mat = if tint && img.premul {
            Mat::PremulTint
        } else {
            self.mat_for(img.premul)
        };
        self.use_mat(mat);
        let (p, angle) = self.corner(dx, dy);
        let c = if tint {
            color
        } else {
            Color::new(color.r, color.g, color.b, color.a * self.state.alpha)
        };
        draw_texture_ex(
            &img.tex,
            p.x,
            p.y,
            c,
            DrawTextureParams {
                dest_size: Some(vec2(dw, dh)),
                rotation: angle,
                pivot: Some(p),
                flip_x: img.flip_x,
                flip_y: img.flip_y,
                source: None,
            },
        );
    }

    /// `fillRect` with a straight-alpha colour.
    pub fn fill_rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: Color) {
        let mat = self.mat_for(false);
        self.use_mat(mat);
        let (p, angle) = self.corner(x, y);
        let c = Color::new(color.r, color.g, color.b, color.a * self.state.alpha);
        if angle.abs() < 1e-6 {
            draw_rectangle(p.x, p.y, w, h, c);
        } else {
            draw_rectangle_ex(
                p.x,
                p.y,
                w,
                h,
                DrawRectangleParams {
                    offset: vec2(0.0, 0.0),
                    rotation: angle,
                    color: c,
                },
            );
        }
    }

    /// A filled circle (the minimap's `arc` + `fill`).
    pub fn fill_circle(&mut self, x: f32, y: f32, r: f32, color: Color) {
        let mat = self.mat_for(false);
        self.use_mat(mat);
        let (p, _) = self.corner(x, y);
        let c = Color::new(color.r, color.g, color.b, color.a * self.state.alpha);
        draw_circle(p.x, p.y, r, c);
    }

    /// A line whose colour fades from `from` to `to` (a canvas linear
    /// gradient stroked along the line), `width` wide, butt ends.
    pub fn gradient_line(&mut self, a: Vec2, b: Vec2, width: f32, from: Color, to: Color) {
        let mat = self.mat_for(false);
        self.use_mat(mat);
        let pa = self.state.m.transform_point2(a);
        let pb = self.state.m.transform_point2(b);
        let d = pb - pa;
        let len = d.length();
        if len < 1e-6 {
            return;
        }
        let n = vec2(-d.y, d.x) / len * (width / 2.0);
        let ga = self.state.alpha;
        let ca = Color::new(from.r, from.g, from.b, from.a * ga);
        let cb = Color::new(to.r, to.g, to.b, to.a * ga);
        let v = |p: Vec2, c: Color| Vertex::new(p.x, p.y, 0.0, 0.0, 0.0, c);
        draw_mesh(&Mesh {
            vertices: vec![v(pa + n, ca), v(pa - n, ca), v(pb - n, cb), v(pb + n, cb)],
            indices: vec![0, 1, 2, 0, 2, 3],
            texture: None,
        });
    }

    /// Draws `img` as a parallelogram leaning right by `shear` (the
    /// browser's synthetic italic).
    pub fn draw_image_sheared(
        &mut self,
        img: &Image,
        dx: f32,
        dy: f32,
        dw: f32,
        dh: f32,
        shear: f32,
    ) {
        let mat = self.mat_for(img.premul);
        self.use_mat(mat);
        let m = self.state.m;
        let ga = self.state.alpha;
        let c = Color::new(1.0, 1.0, 1.0, ga);
        let (v0, v1) = if img.flip_y { (1.0, 0.0) } else { (0.0, 1.0) };
        let mid = dy + dh / 2.0;
        let pt = |x: f32, y: f32| m.transform_point2(vec2(x - (y - mid) * shear, y));
        let tl = pt(dx, dy);
        let tr = pt(dx + dw, dy);
        let br = pt(dx + dw, dy + dh);
        let bl = pt(dx, dy + dh);
        let v = |p: Vec2, u: f32, vv: f32| Vertex::new(p.x, p.y, 0.0, u, vv, c);
        draw_mesh(&Mesh {
            vertices: vec![
                v(tl, 0.0, v0),
                v(tr, 1.0, v0),
                v(br, 1.0, v1),
                v(bl, 0.0, v1),
            ],
            indices: vec![0, 1, 2, 0, 2, 3],
            texture: Some(img.tex.clone()),
        });
    }

    /// Sets the material for raw macroquad calls (text) in the current
    /// blend and target.
    pub fn flush_premul_text(&mut self) {
        let m = self.mat_for(false);
        self.use_mat(m);
    }

    /// Forgets which material is bound, after other code drew.
    pub fn invalidate(&mut self) {
        self.current = Mat::Unknown;
    }
}

/// Parses `#rrggbb` (and `#rgb`) into a colour.
#[must_use]
pub fn hex(s: &str) -> Color {
    let s = s.trim_start_matches('#');
    let p = |i: usize, n: usize| u8::from_str_radix(&s[i..i + n], 16).unwrap_or(255);
    match s.len() {
        3 => {
            let d = |i: usize| f32::from(p(i, 1) * 17) / 255.0;
            Color::new(d(0), d(1), d(2), 1.0)
        }
        6 => Color::from_rgba(p(0, 2), p(2, 2), p(4, 2), 255),
        _ => WHITE,
    }
}

/// KRP's `shadeColor`: each channel times `(100 + percent) / 100`.
#[must_use]
pub fn shade(s: &str, percent: f32) -> Color {
    let c = hex(s);
    let f = |v: f32| ((v * 255.0 * (100.0 + percent) / 100.0).round().min(255.0)) / 255.0;
    Color::new(f(c.r), f(c.g), f(c.b), 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_parse_and_shade_like_krp() {
        let c = hex("#d95151");
        assert_eq!((c.r * 255.0).round() as u8, 0xd9);
        let s = shade("#ffffff", -18.0);
        assert_eq!((s.r * 255.0).round() as u8, 209);
    }
}
