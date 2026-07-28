use std::collections::{BTreeMap, HashMap, HashSet};
use std::f64::consts::{PI, TAU};
use std::fmt::Write as _;

const MAX_PRIMITIVES: usize = 1_000_000;
const MAX_BLOCK_DEPTH: usize = 24;

#[derive(Clone, Copy, Debug, Default)]
struct Point {
    x: f64,
    y: f64,
}

impl Point {
    fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

#[derive(Clone, Copy, Debug)]
struct Affine {
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    e: f64,
    f: f64,
}

impl Affine {
    const IDENTITY: Self = Self {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        e: 0.0,
        f: 0.0,
    };

    fn translation(x: f64, y: f64) -> Self {
        Self {
            e: x,
            f: y,
            ..Self::IDENTITY
        }
    }

    fn scale(x: f64, y: f64) -> Self {
        Self {
            a: x,
            d: y,
            ..Self::IDENTITY
        }
    }

    fn rotation(degrees: f64) -> Self {
        let radians = degrees.to_radians();
        let (sin, cos) = radians.sin_cos();
        Self {
            a: cos,
            b: sin,
            c: -sin,
            d: cos,
            e: 0.0,
            f: 0.0,
        }
    }

    /// Returns `self ∘ rhs`.
    fn then(self, rhs: Self) -> Self {
        Self {
            a: self.a * rhs.a + self.c * rhs.b,
            b: self.b * rhs.a + self.d * rhs.b,
            c: self.a * rhs.c + self.c * rhs.d,
            d: self.b * rhs.c + self.d * rhs.d,
            e: self.a * rhs.e + self.c * rhs.f + self.e,
            f: self.b * rhs.e + self.d * rhs.f + self.f,
        }
    }

    fn apply(self, point: Point) -> Point {
        Point::new(
            self.a * point.x + self.c * point.y + self.e,
            self.b * point.x + self.d * point.y + self.f,
        )
    }

    fn x_axis_angle(self) -> f64 {
        self.b.atan2(self.a).to_degrees()
    }

    fn average_scale(self) -> f64 {
        let x = self.a.hypot(self.b);
        let y = self.c.hypot(self.d);
        ((x + y) * 0.5).abs()
    }
}

#[derive(Debug)]
pub enum Primitive {
    Polyline {
        points: Vec<(f64, f64)>,
        closed: bool,
        filled: bool,
        color: CadColor,
    },
    Text {
        x: f64,
        y: f64,
        height: f64,
        rotation: f64,
        width_factor: f64,
        anchor: TextAnchor,
        baseline: TextBaseline,
        color: CadColor,
        value: String,
    },
    Point {
        x: f64,
        y: f64,
        color: CadColor,
    },
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CadColor(u32);

impl CadColor {
    const WHITE: Self = Self(0xE5E7EB);

    fn from_rgb(rgb: u32) -> Self {
        Self(rgb & 0x00FF_FFFF)
    }

    fn hex(self) -> String {
        format!("#{:06X}", self.0)
    }
}

#[derive(Clone, Copy, Debug)]
pub enum TextAnchor {
    Start,
    Middle,
    End,
}

#[derive(Clone, Copy, Debug)]
pub enum TextBaseline {
    Baseline,
    Top,
    Middle,
    Bottom,
}

#[derive(Debug)]
pub struct Scene {
    pub primitives: Vec<Primitive>,
    bounds: Bounds,
}

#[derive(Clone, Copy, Debug)]
struct Bounds {
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
}

impl Bounds {
    fn empty() -> Self {
        Self {
            min_x: f64::INFINITY,
            min_y: f64::INFINITY,
            max_x: f64::NEG_INFINITY,
            max_y: f64::NEG_INFINITY,
        }
    }

    fn add(&mut self, point: Point) {
        if point.x.is_finite() && point.y.is_finite() {
            self.min_x = self.min_x.min(point.x);
            self.min_y = self.min_y.min(point.y);
            self.max_x = self.max_x.max(point.x);
            self.max_y = self.max_y.max(point.y);
        }
    }

    fn valid(self) -> bool {
        self.min_x.is_finite()
            && self.min_y.is_finite()
            && self.max_x > self.min_x
            && self.max_y > self.min_y
    }
}

#[derive(Clone, Debug)]
struct Pair {
    code: i32,
    value: String,
}

#[derive(Clone, Debug)]
struct RawEntity {
    kind: String,
    pairs: Vec<Pair>,
    children: Vec<RawEntity>,
}

impl RawEntity {
    fn text(&self, code: i32) -> Option<&str> {
        self.pairs
            .iter()
            .find(|pair| pair.code == code)
            .map(|pair| pair.value.as_str())
    }

    fn number(&self, code: i32, default: f64) -> f64 {
        self.text(code)
            .and_then(|value| value.trim().parse().ok())
            .unwrap_or(default)
    }

    fn integer(&self, code: i32, default: i32) -> i32 {
        self.text(code)
            .and_then(|value| value.trim().parse().ok())
            .unwrap_or(default)
    }

    fn point(&self, x_code: i32, y_code: i32) -> Point {
        Point::new(self.number(x_code, 0.0), self.number(y_code, 0.0))
    }

    fn points(&self, x_code: i32, y_code: i32) -> Vec<Point> {
        let xs = self
            .pairs
            .iter()
            .filter(|pair| pair.code == x_code)
            .filter_map(|pair| pair.value.trim().parse::<f64>().ok());
        let ys = self
            .pairs
            .iter()
            .filter(|pair| pair.code == y_code)
            .filter_map(|pair| pair.value.trim().parse::<f64>().ok());
        xs.zip(ys).map(|(x, y)| Point::new(x, y)).collect()
    }
}

#[derive(Clone, Debug)]
struct Block {
    base: Point,
    entities: Vec<RawEntity>,
}

#[derive(Clone, Copy, Debug)]
struct LayerStyle {
    color: CadColor,
    visible: bool,
}

pub fn parse(input: &str) -> Result<Scene, String> {
    let pairs = parse_pairs(input)?;
    let mut blocks = HashMap::new();
    let mut layers = HashMap::new();
    let mut entities = Vec::new();
    let mut initial_view = None;
    let mut index = 0;

    while index < pairs.len() {
        if is_pair(&pairs[index], 0, "SECTION")
            && pairs.get(index + 1).is_some_and(|pair| pair.code == 2)
        {
            let section_name = pairs[index + 1].value.trim().to_ascii_uppercase();
            index += 2;
            let start = index;
            while index < pairs.len() && !is_pair(&pairs[index], 0, "ENDSEC") {
                index += 1;
            }
            let section = &pairs[start..index];
            if section_name == "ENTITIES" {
                entities = read_entities(section);
            } else if section_name == "BLOCKS" {
                blocks = read_blocks(section);
            } else if section_name == "TABLES" {
                layers = read_layers(section);
                initial_view = read_active_view(section);
            }
        }
        index += 1;
    }

    initial_view = preferred_title_view(&entities).or(initial_view);
    if let Ok(value) = std::env::var("CADVIEWER_VIEW_BOUNDS") {
        let values: Vec<_> = value
            .split(',')
            .filter_map(|part| part.trim().parse::<f64>().ok())
            .collect();
        if let [min_x, min_y, max_x, max_y] = values.as_slice()
            && max_x > min_x
            && max_y > min_y
        {
            initial_view = Some(Bounds {
                min_x: *min_x,
                min_y: *min_y,
                max_x: *max_x,
                max_y: *max_y,
            });
        }
    }

    let mut primitives = Vec::new();
    let mut active_blocks = HashSet::new();
    emit_root_entities(
        &entities,
        &blocks,
        &layers,
        initial_view,
        Affine::IDENTITY,
        CadColor::WHITE,
        &mut active_blocks,
        0,
        &mut primitives,
    );

    let mut bounds = initial_view.unwrap_or_else(Bounds::empty);
    for primitive in &primitives {
        if initial_view.is_some() {
            break;
        }
        match primitive {
            Primitive::Polyline { points, .. } => {
                for &(x, y) in points {
                    bounds.add(Point::new(x, y));
                }
            }
            Primitive::Text {
                x,
                y,
                height,
                rotation,
                width_factor,
                value,
                ..
            } => {
                let text_width =
                    height.abs() * value.chars().count() as f64 * 0.65 * width_factor.abs();
                let radians = rotation.to_radians();
                let (sin, cos) = radians.sin_cos();
                for corner in [
                    Point::new(0.0, 0.0),
                    Point::new(text_width, 0.0),
                    Point::new(0.0, height.abs()),
                    Point::new(text_width, height.abs()),
                ] {
                    bounds.add(Point::new(
                        x + corner.x * cos - corner.y * sin,
                        y + corner.x * sin + corner.y * cos,
                    ));
                }
            }
            Primitive::Point { x, y, .. } => bounds.add(Point::new(*x, *y)),
        }
    }

    if !bounds.valid() {
        return Err("无法确定图纸二维范围".to_owned());
    }

    Ok(Scene { primitives, bounds })
}

impl Scene {
    pub fn to_svg(&self) -> String {
        let raw_width = (self.bounds.max_x - self.bounds.min_x).max(f64::EPSILON);
        let raw_height = (self.bounds.max_y - self.bounds.min_y).max(f64::EPSILON);
        let raw_long_side = raw_width.max(raw_height);
        let margin = raw_long_side * 0.025;
        let scale = 1000.0 / (raw_long_side + margin * 2.0);
        let width = (raw_width + margin * 2.0) * scale;
        let height = (raw_height + margin * 2.0) * scale;
        let stroke_width = 0.72_f64;
        let point_radius = 1.5_f64;

        let map = |x: f64, y: f64| {
            (
                (x - self.bounds.min_x + margin) * scale,
                (self.bounds.max_y - y + margin) * scale,
            )
        };

        let mut svg = String::with_capacity(self.primitives.len() * 96);
        svg.push_str(r#"<?xml version="1.0" encoding="UTF-8"?>"#);
        svg.push('\n');
        svg.push_str(&format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width:.4}" height="{height:.4}" viewBox="0 0 {width:.4} {height:.4}">"#
        ));
        svg.push('\n');
        svg.push_str(r##"<rect width="100%" height="100%" fill="#202830"/>"##);
        svg.push('\n');

        let mut paths: BTreeMap<(CadColor, bool), String> = BTreeMap::new();
        for primitive in &self.primitives {
            match primitive {
                Primitive::Polyline {
                    points,
                    closed,
                    filled,
                    color,
                } if points.len() >= 2 => {
                    let data = paths.entry((*color, *filled)).or_default();
                    for (index, &(x, y)) in points.iter().enumerate() {
                        let (x, y) = map(x, y);
                        let command = if index == 0 { 'M' } else { 'L' };
                        let _ = write!(data, "{command}{x:.3},{y:.3}");
                    }
                    if *closed {
                        data.push('Z');
                    }
                }
                Primitive::Point { x, y, color } => {
                    let (x, y) = map(*x, *y);
                    let data = paths.entry((*color, false)).or_default();
                    let _ = write!(
                        data,
                        "M{:.3},{y:.3}A{point_radius},{point_radius} 0 1 0 {:.3},{y:.3}A{point_radius},{point_radius} 0 1 0 {:.3},{y:.3}",
                        x - point_radius,
                        x + point_radius,
                        x - point_radius
                    );
                }
                _ => {}
            }
        }

        for ((color, filled), data) in paths {
            let color = color.hex();
            if filled {
                let _ = writeln!(
                    svg,
                    r#"<path d="{data}" fill="{color}" stroke="{color}" stroke-width="{stroke_width}" stroke-linejoin="round"/>"#
                );
            } else {
                let _ = writeln!(
                    svg,
                    r#"<path d="{data}" fill="none" stroke="{color}" stroke-width="{stroke_width}" stroke-linecap="round" stroke-linejoin="round"/>"#
                );
            }
        }

        for primitive in &self.primitives {
            if let Primitive::Text {
                x,
                y,
                height: text_height,
                rotation,
                width_factor,
                anchor,
                baseline,
                color,
                value,
            } = primitive
            {
                let (x, y) = map(*x, *y);
                let font_size = (text_height.abs() * scale).max(2.0);
                let escaped = escape_xml(&plain_text(value));
                if escaped.trim().is_empty() {
                    continue;
                }
                let text_anchor = match anchor {
                    TextAnchor::Start => "start",
                    TextAnchor::Middle => "middle",
                    TextAnchor::End => "end",
                };
                let baseline = match baseline {
                    TextBaseline::Baseline => "alphabetic",
                    TextBaseline::Top => "text-before-edge",
                    TextBaseline::Middle => "central",
                    TextBaseline::Bottom => "text-after-edge",
                };
                let color = color.hex();
                svg.push_str(&format!(
                    r#"<text x="{x:.4}" y="{y:.4}" font-family="SimSun, Microsoft YaHei, Arial, sans-serif" font-size="{font_size:.4}" fill="{color}" text-anchor="{text_anchor}" dominant-baseline="{baseline}" transform="translate({x:.4} {y:.4}) rotate({:.4}) scale({:.4} 1) translate({:.4} {:.4})">{escaped}</text>"#,
                    -*rotation,
                    width_factor.max(0.01),
                    -x,
                    -y
                ));
                svg.push('\n');
            }
        }
        svg.push_str("</svg>\n");
        svg
    }
}

fn parse_pairs(input: &str) -> Result<Vec<Pair>, String> {
    let normalized = input.replace("\r\n", "\n").replace('\r', "\n");
    let mut lines = normalized.lines();
    let mut pairs = Vec::new();
    while let Some(code) = lines.next() {
        let Some(value) = lines.next() else {
            return Err("DXF 末尾缺少组值".to_owned());
        };
        let code = code
            .trim()
            .parse::<i32>()
            .map_err(|_| format!("无效的 DXF 组码：{code}"))?;
        pairs.push(Pair {
            code,
            value: value.trim_end().to_owned(),
        });
    }
    Ok(pairs)
}

fn is_pair(pair: &Pair, code: i32, value: &str) -> bool {
    pair.code == code && pair.value.trim().eq_ignore_ascii_case(value)
}

fn read_entities(pairs: &[Pair]) -> Vec<RawEntity> {
    let mut entities = Vec::new();
    let mut index = 0;
    while index < pairs.len() {
        if pairs[index].code != 0 {
            index += 1;
            continue;
        }
        let kind = pairs[index].value.trim().to_ascii_uppercase();
        index += 1;
        let start = index;
        while index < pairs.len() && pairs[index].code != 0 {
            index += 1;
        }
        let mut entity = RawEntity {
            kind: kind.clone(),
            pairs: pairs[start..index].to_vec(),
            children: Vec::new(),
        };

        if kind == "POLYLINE" {
            while index < pairs.len() && is_pair(&pairs[index], 0, "VERTEX") {
                index += 1;
                let child_start = index;
                while index < pairs.len() && pairs[index].code != 0 {
                    index += 1;
                }
                entity.children.push(RawEntity {
                    kind: "VERTEX".to_owned(),
                    pairs: pairs[child_start..index].to_vec(),
                    children: Vec::new(),
                });
            }
            if index < pairs.len() && is_pair(&pairs[index], 0, "SEQEND") {
                index += 1;
                while index < pairs.len() && pairs[index].code != 0 {
                    index += 1;
                }
            }
        }
        entities.push(entity);
    }
    entities
}

fn read_blocks(pairs: &[Pair]) -> HashMap<String, Block> {
    let mut blocks = HashMap::new();
    let mut index = 0;
    while index < pairs.len() {
        if !is_pair(&pairs[index], 0, "BLOCK") {
            index += 1;
            continue;
        }
        index += 1;
        let header_start = index;
        while index < pairs.len() && pairs[index].code != 0 {
            index += 1;
        }
        let header = RawEntity {
            kind: "BLOCK".to_owned(),
            pairs: pairs[header_start..index].to_vec(),
            children: Vec::new(),
        };
        let body_start = index;
        while index < pairs.len() && !is_pair(&pairs[index], 0, "ENDBLK") {
            index += 1;
        }
        let entities = read_entities(&pairs[body_start..index]);
        let name = header.text(2).unwrap_or_default().trim().to_owned();
        if !name.is_empty() {
            blocks.insert(
                name.to_ascii_uppercase(),
                Block {
                    base: header.point(10, 20),
                    entities,
                },
            );
        }
        while index < pairs.len() && !is_pair(&pairs[index], 0, "BLOCK") {
            index += 1;
        }
    }
    blocks
}

fn read_layers(pairs: &[Pair]) -> HashMap<String, LayerStyle> {
    let mut layers = HashMap::new();
    let mut index = 0;
    while index < pairs.len() {
        if !is_pair(&pairs[index], 0, "LAYER") {
            index += 1;
            continue;
        }
        index += 1;
        let start = index;
        while index < pairs.len() && pairs[index].code != 0 {
            index += 1;
        }
        let layer = RawEntity {
            kind: "LAYER".to_owned(),
            pairs: pairs[start..index].to_vec(),
            children: Vec::new(),
        };
        let name = layer.text(2).unwrap_or_default().trim();
        if name.is_empty() {
            continue;
        }
        let aci = layer.integer(62, 7);
        let true_color = layer.integer(420, 0);
        let flags = layer.integer(70, 0);
        let color = if true_color > 0 {
            CadColor::from_rgb(true_color as u32)
        } else {
            aci_color(aci.unsigned_abs())
        };
        layers.insert(
            name.to_ascii_uppercase(),
            LayerStyle {
                color,
                visible: aci >= 0 && flags & 1 == 0,
            },
        );
    }
    layers
}

fn read_active_view(pairs: &[Pair]) -> Option<Bounds> {
    let records = read_entities(pairs);
    let viewport = records.iter().find(|record| {
        record.kind == "VPORT"
            && record
                .text(2)
                .is_some_and(|name| name.trim().eq_ignore_ascii_case("*ACTIVE"))
    })?;
    let center = viewport.point(12, 22);
    let height = viewport.number(40, 0.0).abs();
    let aspect = viewport.number(41, 1.0).abs();
    if !center.x.is_finite()
        || !center.y.is_finite()
        || !height.is_finite()
        || !aspect.is_finite()
        || height <= f64::EPSILON
        || aspect <= f64::EPSILON
    {
        return None;
    }
    let half_height = height * 0.5;
    let half_width = half_height * aspect;
    Some(Bounds {
        min_x: center.x - half_width,
        min_y: center.y - half_height,
        max_x: center.x + half_width,
        max_y: center.y + half_height,
    })
}

fn preferred_title_view(entities: &[RawEntity]) -> Option<Bounds> {
    let mut selected = None;
    for entity in entities {
        if entity.kind != "MTEXT" {
            continue;
        }
        let mut raw = String::new();
        for pair in &entity.pairs {
            if pair.code == 3 || pair.code == 1 {
                raw.push_str(&pair.value);
            }
        }
        let title = plain_text(&raw);
        let title = title.trim();
        if title.chars().count() < 3
            || title.chars().count() > 30
            || !title.contains('图')
            || title.contains("图例")
            || title.contains("说明")
            || title.contains("图号")
        {
            continue;
        }
        let height = entity.number(40, 0.0).abs();
        if !(100.0..=20_000.0).contains(&height) {
            continue;
        }
        let score = if title.contains("车间")
            && (title.ends_with("暖通图") || title.ends_with("暖通平面图"))
        {
            2_000
        } else if title.ends_with("暖通图") || title.ends_with("暖通平面图") {
            1_000
        } else if title.contains("暖通") && title.chars().count() <= 15 {
            500
        } else if title.contains("暖通") {
            100
        } else if title.contains("装修") {
            80
        } else if title.contains("平面") {
            20
        } else {
            1
        };
        if selected.is_none_or(|(best_score, _, _)| score > best_score) {
            selected = Some((score, entity.point(10, 20), height));
        }
    }

    let (_, title, height) = selected?;
    Some(Bounds {
        min_x: title.x - height * 22.75,
        min_y: title.y - height * 9.5,
        max_x: title.x + height * 28.25,
        max_y: title.y + height * 22.75,
    })
}

fn resolve_color(
    entity: &RawEntity,
    layers: &HashMap<String, LayerStyle>,
    inherited_color: CadColor,
) -> Option<CadColor> {
    if entity.integer(60, 0) == 1 {
        return None;
    }

    let layer_name = entity.text(8).unwrap_or("0").trim().to_ascii_uppercase();
    let layer = layers.get(&layer_name).copied().unwrap_or(LayerStyle {
        color: inherited_color,
        visible: true,
    });
    if !layer.visible {
        return None;
    }
    let layer_color = if layer_name == "0" {
        inherited_color
    } else {
        layer.color
    };

    let true_color = entity.integer(420, 0);
    if true_color > 0 {
        return Some(CadColor::from_rgb(true_color as u32));
    }

    let aci = entity.integer(62, 256);
    if aci < 0 {
        return None;
    }
    Some(match aci {
        0 => inherited_color,
        256 => layer_color,
        value => aci_color(value as u32),
    })
}

fn aci_color(index: u32) -> CadColor {
    match index {
        0 | 7 | 255 | 256 => CadColor::WHITE,
        1 => CadColor::from_rgb(0xFF_3B_30),
        2 => CadColor::from_rgb(0xFF_D6_0A),
        3 => CadColor::from_rgb(0x35_EB_5B),
        4 => CadColor::from_rgb(0x32_D7_EB),
        5 => CadColor::from_rgb(0x3B_82_F6),
        6 => CadColor::from_rgb(0xF0_4D_FC),
        8 => CadColor::from_rgb(0x80_8791),
        9 => CadColor::from_rgb(0xC8_CDD4),
        10..=249 => {
            let offset = index - 10;
            let hue = (offset / 10) as f64 * 15.0;
            let shade = offset % 10;
            let saturation = if shade < 5 { 1.0 } else { 0.5 };
            let values = [1.0, 0.65, 0.5, 0.3, 0.15];
            let value = values[(shade % 5) as usize];
            CadColor::from_rgb(hsv_to_rgb(hue, saturation, value))
        }
        250..=254 => {
            let grays = [0x33, 0x5B, 0x84, 0xAD, 0xD6];
            let gray = grays[(index - 250) as usize];
            CadColor::from_rgb((gray << 16) | (gray << 8) | gray)
        }
        _ => CadColor::WHITE,
    }
}

fn hsv_to_rgb(hue: f64, saturation: f64, value: f64) -> u32 {
    let chroma = value * saturation;
    let section = (hue / 60.0) % 6.0;
    let x = chroma * (1.0 - (section % 2.0 - 1.0).abs());
    let (r, g, b) = match section.floor() as i32 {
        0 => (chroma, x, 0.0),
        1 => (x, chroma, 0.0),
        2 => (0.0, chroma, x),
        3 => (0.0, x, chroma),
        4 => (x, 0.0, chroma),
        _ => (chroma, 0.0, x),
    };
    let m = value - chroma;
    let channel = |component: f64| ((component + m) * 255.0).round() as u32;
    (channel(r) << 16) | (channel(g) << 8) | channel(b)
}

fn text_anchor(horizontal: i32) -> TextAnchor {
    match horizontal {
        1 | 3 | 4 | 5 => TextAnchor::Middle,
        2 => TextAnchor::End,
        _ => TextAnchor::Start,
    }
}

fn text_baseline(vertical: i32) -> TextBaseline {
    match vertical {
        1 => TextBaseline::Bottom,
        2 => TextBaseline::Middle,
        3 => TextBaseline::Top,
        _ => TextBaseline::Baseline,
    }
}

fn mtext_anchor(attachment: i32) -> TextAnchor {
    match attachment % 3 {
        2 => TextAnchor::Middle,
        0 => TextAnchor::End,
        _ => TextAnchor::Start,
    }
}

fn mtext_baseline(attachment: i32) -> TextBaseline {
    match attachment {
        1..=3 => TextBaseline::Top,
        4..=6 => TextBaseline::Middle,
        _ => TextBaseline::Bottom,
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_entities(
    entities: &[RawEntity],
    blocks: &HashMap<String, Block>,
    layers: &HashMap<String, LayerStyle>,
    clip_bounds: Option<Bounds>,
    transform: Affine,
    inherited_color: CadColor,
    active_blocks: &mut HashSet<String>,
    depth: usize,
    output: &mut Vec<Primitive>,
) {
    if depth > MAX_BLOCK_DEPTH || output.len() >= MAX_PRIMITIVES {
        return;
    }
    for entity in entities {
        if output.len() >= MAX_PRIMITIVES {
            break;
        }
        emit_entity(
            entity,
            blocks,
            layers,
            clip_bounds,
            transform,
            inherited_color,
            active_blocks,
            depth,
            output,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_root_entities(
    entities: &[RawEntity],
    blocks: &HashMap<String, Block>,
    layers: &HashMap<String, LayerStyle>,
    initial_view: Option<Bounds>,
    transform: Affine,
    inherited_color: CadColor,
    active_blocks: &mut HashSet<String>,
    depth: usize,
    output: &mut Vec<Primitive>,
) {
    for entity in entities {
        if output.len() >= MAX_PRIMITIVES {
            break;
        }
        if entity.integer(67, 0) != 0 {
            continue;
        }
        emit_entity(
            entity,
            blocks,
            layers,
            initial_view,
            transform,
            inherited_color,
            active_blocks,
            depth,
            output,
        );
    }
}

fn entity_might_intersect_view(entity: &RawEntity, view: Bounds, transform: Affine) -> bool {
    let padding = (view.max_x - view.min_x).max(view.max_y - view.min_y) * 0.35;
    let padded = Bounds {
        min_x: view.min_x - padding,
        min_y: view.min_y - padding,
        max_x: view.max_x + padding,
        max_y: view.max_y + padding,
    };

    if matches!(
        entity.kind.as_str(),
        "TEXT" | "ATTRIB" | "ATTDEF" | "MTEXT" | "POINT"
    ) {
        let point = transform.apply(text_location(entity));
        return point.x >= padded.min_x
            && point.x <= padded.max_x
            && point.y >= padded.min_y
            && point.y <= padded.max_y;
    }

    let point_sets: &[(i32, i32)] = match entity.kind.as_str() {
        "LWPOLYLINE" | "LEADER" => &[(10, 20)],
        "SPLINE" => &[(10, 20), (11, 21)],
        "MLINE" => &[(11, 21)],
        "LINE" | "SOLID" | "TRACE" | "3DFACE" => &[(10, 20), (11, 21), (12, 22), (13, 23)],
        _ => &[(10, 20)],
    };
    let mut bounds = Bounds::empty();
    let mut point_count = 0;
    for &(x_code, y_code) in point_sets {
        for point in entity.points(x_code, y_code) {
            bounds.add(transform.apply(point));
            point_count += 1;
        }
    }
    if point_count == 0 {
        return true;
    }
    bounds.max_x >= padded.min_x
        && bounds.min_x <= padded.max_x
        && bounds.max_y >= padded.min_y
        && bounds.min_y <= padded.max_y
}

fn text_location(entity: &RawEntity) -> Point {
    if entity.kind == "MTEXT" {
        return entity.point(10, 20);
    }
    let horizontal = entity.integer(72, 0);
    let vertical = entity.integer(73, 0);
    let alignment = entity.point(11, 21);
    let insertion = entity.point(10, 20);
    if (horizontal != 0 || vertical != 0)
        && (alignment.x != 0.0 || alignment.y != 0.0 || insertion.x == 0.0 && insertion.y == 0.0)
    {
        alignment
    } else {
        insertion
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_entity(
    entity: &RawEntity,
    blocks: &HashMap<String, Block>,
    layers: &HashMap<String, LayerStyle>,
    clip_bounds: Option<Bounds>,
    transform: Affine,
    inherited_color: CadColor,
    active_blocks: &mut HashSet<String>,
    depth: usize,
    output: &mut Vec<Primitive>,
) {
    if !matches!(entity.kind.as_str(), "INSERT" | "DIMENSION")
        && let Some(view) = clip_bounds
        && !entity_might_intersect_view(entity, view, transform)
    {
        return;
    }
    let Some(color) = resolve_color(entity, layers, inherited_color) else {
        return;
    };
    match entity.kind.as_str() {
        "LINE" => push_polyline(
            output,
            vec![
                transform.apply(entity.point(10, 20)),
                transform.apply(entity.point(11, 21)),
            ],
            false,
            false,
            color,
        ),
        "CIRCLE" => {
            let center = entity.point(10, 20);
            let radius = entity.number(40, 0.0).abs();
            push_polyline(
                output,
                sample_curve(0.0, TAU, 96, |angle| {
                    transform.apply(Point::new(
                        center.x + radius * angle.cos(),
                        center.y + radius * angle.sin(),
                    ))
                }),
                true,
                false,
                color,
            );
        }
        "ARC" => {
            let center = entity.point(10, 20);
            let radius = entity.number(40, 0.0).abs();
            let start = entity.number(50, 0.0).to_radians();
            let mut end = entity.number(51, 360.0).to_radians();
            while end <= start {
                end += TAU;
            }
            let segments = (((end - start).abs() / TAU) * 96.0).ceil() as usize;
            push_polyline(
                output,
                sample_curve(start, end, segments.max(8), |angle| {
                    transform.apply(Point::new(
                        center.x + radius * angle.cos(),
                        center.y + radius * angle.sin(),
                    ))
                }),
                false,
                false,
                color,
            );
        }
        "ELLIPSE" => {
            let center = entity.point(10, 20);
            let major = entity.point(11, 21);
            let ratio = entity.number(40, 1.0);
            let start = entity.number(41, 0.0);
            let mut end = entity.number(42, TAU);
            while end <= start {
                end += TAU;
            }
            let minor = Point::new(-major.y * ratio, major.x * ratio);
            let closed = (end - start - TAU).abs() < 1.0e-6;
            push_polyline(
                output,
                sample_curve(start, end, 96, |angle| {
                    transform.apply(Point::new(
                        center.x + major.x * angle.cos() + minor.x * angle.sin(),
                        center.y + major.y * angle.cos() + minor.y * angle.sin(),
                    ))
                }),
                closed,
                false,
                color,
            );
        }
        "LWPOLYLINE" => {
            let vertices = lwpolyline_vertices(entity);
            let closed = entity.integer(70, 0) & 1 != 0;
            let points = bulged_polyline(&vertices, closed)
                .into_iter()
                .map(|point| transform.apply(point))
                .collect();
            push_polyline(output, points, closed, false, color);
        }
        "POLYLINE" => {
            let points: Vec<_> = entity
                .children
                .iter()
                .filter(|vertex| vertex.integer(70, 0) & 128 == 0)
                .map(|vertex| transform.apply(vertex.point(10, 20)))
                .collect();
            push_polyline(output, points, entity.integer(70, 0) & 1 != 0, false, color);
        }
        "SPLINE" => {
            let mut points = entity.points(11, 21);
            if points.len() < 2 {
                points = entity.points(10, 20);
            }
            push_polyline(
                output,
                points
                    .into_iter()
                    .map(|point| transform.apply(point))
                    .collect(),
                entity.integer(70, 0) & 1 != 0,
                false,
                color,
            );
        }
        "SOLID" | "TRACE" | "3DFACE" => {
            let points = [10, 11, 13, 12]
                .into_iter()
                .map(|code| transform.apply(entity.point(code, code + 10)))
                .collect();
            push_polyline(output, points, true, entity.kind != "3DFACE", color);
        }
        "POINT" => {
            let point = transform.apply(entity.point(10, 20));
            output.push(Primitive::Point {
                x: point.x,
                y: point.y,
                color,
            });
        }
        "TEXT" | "ATTRIB" | "ATTDEF" => {
            let horizontal = entity.integer(72, 0);
            let vertical = entity.integer(73, 0);
            emit_text(
                output,
                transform,
                text_location(entity),
                entity.number(40, 1.0),
                entity.number(50, 0.0),
                entity.number(41, 1.0),
                text_anchor(horizontal),
                text_baseline(vertical),
                color,
                entity.text(1).unwrap_or_default(),
            );
        }
        "MTEXT" => {
            let mut value = String::new();
            for pair in &entity.pairs {
                if pair.code == 3 || pair.code == 1 {
                    value.push_str(&pair.value);
                }
            }
            let attachment = entity.integer(71, 1).clamp(1, 9);
            let rotation = if entity.text(50).is_some() {
                entity.number(50, 0.0)
            } else {
                entity
                    .number(21, 0.0)
                    .atan2(entity.number(11, 1.0))
                    .to_degrees()
            };
            emit_text(
                output,
                transform,
                entity.point(10, 20),
                entity.number(40, 1.0),
                rotation,
                1.0,
                mtext_anchor(attachment),
                mtext_baseline(attachment),
                color,
                &value,
            );
        }
        "LEADER" | "MLINE" => {
            let (x_code, y_code) = if entity.kind == "MLINE" {
                (11, 21)
            } else {
                (10, 20)
            };
            push_polyline(
                output,
                entity
                    .points(x_code, y_code)
                    .into_iter()
                    .map(|point| transform.apply(point))
                    .collect(),
                false,
                false,
                color,
            );
        }
        "INSERT" => emit_insert(
            entity,
            blocks,
            layers,
            clip_bounds,
            transform,
            color,
            active_blocks,
            depth,
            output,
        ),
        "DIMENSION" => {
            if let Some(name) = entity.text(2) {
                emit_block(
                    name,
                    blocks,
                    layers,
                    clip_bounds,
                    transform,
                    color,
                    active_blocks,
                    depth + 1,
                    output,
                );
            }
        }
        _ => {}
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_text(
    output: &mut Vec<Primitive>,
    transform: Affine,
    location: Point,
    height: f64,
    rotation: f64,
    width_factor: f64,
    anchor: TextAnchor,
    baseline: TextBaseline,
    color: CadColor,
    value: &str,
) {
    let point = transform.apply(location);
    output.push(Primitive::Text {
        x: point.x,
        y: point.y,
        height: height * transform.average_scale(),
        rotation: rotation + transform.x_axis_angle(),
        width_factor,
        anchor,
        baseline,
        color,
        value: value.to_owned(),
    });
}

#[allow(clippy::too_many_arguments)]
fn emit_insert(
    entity: &RawEntity,
    blocks: &HashMap<String, Block>,
    layers: &HashMap<String, LayerStyle>,
    clip_bounds: Option<Bounds>,
    parent: Affine,
    inherited_color: CadColor,
    active_blocks: &mut HashSet<String>,
    depth: usize,
    output: &mut Vec<Primitive>,
) {
    let Some(name) = entity.text(2) else {
        return;
    };
    let location = entity.point(10, 20);
    let sx = entity.number(41, 1.0);
    let sy = entity.number(42, 1.0);
    let rotation = entity.number(50, 0.0);
    let rows = entity.integer(71, 1).clamp(1, 1024);
    let columns = entity.integer(70, 1).clamp(1, 1024);
    let row_spacing = entity.number(45, 0.0);
    let column_spacing = entity.number(44, 0.0);

    for row in 0..rows {
        for column in 0..columns {
            let local = Affine::translation(location.x, location.y)
                .then(Affine::rotation(rotation))
                .then(Affine::scale(sx, sy))
                .then(Affine::translation(
                    column as f64 * column_spacing,
                    row as f64 * row_spacing,
                ));
            emit_block(
                name,
                blocks,
                layers,
                clip_bounds,
                parent.then(local),
                inherited_color,
                active_blocks,
                depth + 1,
                output,
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_block(
    name: &str,
    blocks: &HashMap<String, Block>,
    layers: &HashMap<String, LayerStyle>,
    clip_bounds: Option<Bounds>,
    transform: Affine,
    inherited_color: CadColor,
    active_blocks: &mut HashSet<String>,
    depth: usize,
    output: &mut Vec<Primitive>,
) {
    let key = name.trim().to_ascii_uppercase();
    let Some(block) = blocks.get(&key) else {
        return;
    };
    if depth > MAX_BLOCK_DEPTH || !active_blocks.insert(key.clone()) {
        return;
    }
    let transform = transform.then(Affine::translation(-block.base.x, -block.base.y));
    emit_entities(
        &block.entities,
        blocks,
        layers,
        clip_bounds,
        transform,
        inherited_color,
        active_blocks,
        depth,
        output,
    );
    active_blocks.remove(&key);
}

fn push_polyline(
    output: &mut Vec<Primitive>,
    points: Vec<Point>,
    closed: bool,
    filled: bool,
    color: CadColor,
) {
    let points: Vec<_> = points
        .into_iter()
        .filter(|point| point.x.is_finite() && point.y.is_finite())
        .map(|point| (point.x, point.y))
        .collect();
    if points.len() >= 2 {
        output.push(Primitive::Polyline {
            points,
            closed,
            filled,
            color,
        });
    }
}

fn sample_curve(
    start: f64,
    end: f64,
    segments: usize,
    mut point_at: impl FnMut(f64) -> Point,
) -> Vec<Point> {
    let segments = segments.clamp(2, 512);
    (0..=segments)
        .map(|index| {
            let t = index as f64 / segments as f64;
            point_at(start + (end - start) * t)
        })
        .collect()
}

#[derive(Clone, Copy)]
struct BulgeVertex {
    point: Point,
    bulge: f64,
}

fn lwpolyline_vertices(entity: &RawEntity) -> Vec<BulgeVertex> {
    let mut vertices: Vec<BulgeVertex> = Vec::new();
    for pair in &entity.pairs {
        match pair.code {
            10 => {
                if let Ok(x) = pair.value.trim().parse::<f64>() {
                    vertices.push(BulgeVertex {
                        point: Point::new(x, 0.0),
                        bulge: 0.0,
                    });
                }
            }
            20 => {
                if let (Some(vertex), Ok(y)) =
                    (vertices.last_mut(), pair.value.trim().parse::<f64>())
                {
                    vertex.point.y = y;
                }
            }
            42 => {
                if let (Some(vertex), Ok(bulge)) =
                    (vertices.last_mut(), pair.value.trim().parse::<f64>())
                {
                    vertex.bulge = bulge;
                }
            }
            _ => {}
        }
    }
    vertices
}

fn bulged_polyline(vertices: &[BulgeVertex], closed: bool) -> Vec<Point> {
    if vertices.len() < 2 {
        return vertices.iter().map(|vertex| vertex.point).collect();
    }

    let segment_count = if closed {
        vertices.len()
    } else {
        vertices.len() - 1
    };
    let mut output = Vec::new();
    for index in 0..segment_count {
        let current = vertices[index];
        let next = vertices[(index + 1) % vertices.len()];
        if index == 0 {
            output.push(current.point);
        }
        if current.bulge.abs() < 1.0e-10 {
            output.push(next.point);
            continue;
        }

        let dx = next.point.x - current.point.x;
        let dy = next.point.y - current.point.y;
        let chord = dx.hypot(dy);
        if chord <= f64::EPSILON {
            continue;
        }
        let b = current.bulge;
        let center = Point::new(
            (current.point.x + next.point.x) * 0.5 - dy * (1.0 - b * b) / (4.0 * b),
            (current.point.y + next.point.y) * 0.5 + dx * (1.0 - b * b) / (4.0 * b),
        );
        let radius = chord * (1.0 + b * b) / (4.0 * b.abs());
        let start = (current.point.y - center.y).atan2(current.point.x - center.x);
        let sweep = 4.0 * b.atan();
        let segments = ((sweep.abs() / (PI / 18.0)).ceil() as usize).clamp(2, 128);
        for step in 1..=segments {
            let angle = start + sweep * step as f64 / segments as f64;
            output.push(Point::new(
                center.x + radius * angle.cos(),
                center.y + radius * angle.sin(),
            ));
        }
    }
    output
}

fn plain_text(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    let mut output = String::with_capacity(value.len());
    let mut index = 0;
    while index < chars.len() {
        if chars[index] == '%'
            && chars.get(index + 1) == Some(&'%')
            && let Some(code) = chars.get(index + 2)
        {
            match code.to_ascii_lowercase() {
                'd' => output.push('\u{00B0}'),
                'p' => output.push('\u{00B1}'),
                'c' => output.push('\u{2300}'),
                'u' | 'o' => {}
                _ => {
                    output.push('%');
                    output.push('%');
                    output.push(*code);
                }
            }
            index += 3;
            continue;
        }

        if chars[index] == '\\' {
            let Some(&code) = chars.get(index + 1) else {
                break;
            };
            match code {
                'P' => {
                    output.push('\n');
                    index += 2;
                    continue;
                }
                '~' => {
                    output.push(' ');
                    index += 2;
                    continue;
                }
                '\\' | '{' | '}' => {
                    output.push(code);
                    index += 2;
                    continue;
                }
                'L' | 'l' | 'O' | 'o' | 'K' | 'k' | 'X' => {
                    index += 2;
                    continue;
                }
                'U' if chars.get(index + 2) == Some(&'+') => {
                    let start = index + 3;
                    let mut end = start;
                    while end < chars.len() && end - start < 8 && chars[end].is_ascii_hexdigit() {
                        end += 1;
                    }
                    let hex: String = chars[start..end].iter().collect();
                    if let Ok(value) = u32::from_str_radix(&hex, 16)
                        && let Some(decoded) = char::from_u32(value)
                    {
                        output.push(decoded);
                    }
                    index = end;
                    continue;
                }
                'S' | 's' => {
                    let start = index + 2;
                    let end = chars[start..]
                        .iter()
                        .position(|character| *character == ';')
                        .map(|offset| start + offset)
                        .unwrap_or(chars.len());
                    for character in &chars[start..end] {
                        output.push(match character {
                            '#' | '^' => '/',
                            other => *other,
                        });
                    }
                    index = (end + 1).min(chars.len());
                    continue;
                }
                'A' | 'C' | 'c' | 'F' | 'f' | 'H' | 'h' | 'W' | 'w' | 'Q' | 'q' | 'T' | 't'
                | 'p' => {
                    let start = index + 2;
                    let end = chars[start..]
                        .iter()
                        .position(|character| *character == ';')
                        .map(|offset| start + offset)
                        .unwrap_or(chars.len());
                    index = (end + 1).min(chars.len());
                    continue;
                }
                _ => {
                    output.push(code);
                    index += 2;
                    continue;
                }
            }
        }

        if chars[index] != '{' && chars[index] != '}' {
            output.push(chars[index]);
        }
        index += 1;
    }
    output
}

fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_line_and_normalizes_svg() {
        let dxf =
            "0\nSECTION\n2\nENTITIES\n0\nLINE\n10\n10\n20\n20\n11\n30\n21\n40\n0\nENDSEC\n0\nEOF\n";
        let scene = parse(dxf).unwrap();
        assert_eq!(scene.primitives.len(), 1);
        let svg = scene.to_svg();
        assert!(svg.contains("<path"));
        assert!(svg.contains("viewBox=\"0 0"));
    }

    #[test]
    fn strips_mtext_formatting_without_corrupting_chinese() {
        let value = r"{\W0.8;\C1;备注：测试\P中文%%d\S1#2;}";
        let text = plain_text(value);
        assert_eq!(text, "备注：测试\n中文°1/2");
        assert!(!text.contains("\\W"));
        assert!(!text.contains('{'));
    }

    #[test]
    fn prefers_the_most_specific_drawing_title_for_the_initial_view() {
        let entities = vec![
            RawEntity {
                kind: "MTEXT".to_owned(),
                pairs: vec![
                    Pair {
                        code: 10,
                        value: "100".to_owned(),
                    },
                    Pair {
                        code: 20,
                        value: "200".to_owned(),
                    },
                    Pair {
                        code: 40,
                        value: "10".to_owned(),
                    },
                    Pair {
                        code: 1,
                        value: "图例及相关说明".to_owned(),
                    },
                ],
                children: Vec::new(),
            },
            RawEntity {
                kind: "MTEXT".to_owned(),
                pairs: vec![
                    Pair {
                        code: 10,
                        value: "-3000".to_owned(),
                    },
                    Pair {
                        code: 20,
                        value: "-5000".to_owned(),
                    },
                    Pair {
                        code: 40,
                        value: "300".to_owned(),
                    },
                    Pair {
                        code: 1,
                        value: "五层车间暖通图".to_owned(),
                    },
                ],
                children: Vec::new(),
            },
            RawEntity {
                kind: "MTEXT".to_owned(),
                pairs: vec![
                    Pair {
                        code: 10,
                        value: "9000".to_owned(),
                    },
                    Pair {
                        code: 20,
                        value: "9000".to_owned(),
                    },
                    Pair {
                        code: 40,
                        value: "300".to_owned(),
                    },
                    Pair {
                        code: 1,
                        value: "屋面桁架平面图".to_owned(),
                    },
                ],
                children: Vec::new(),
            },
        ];
        let view = preferred_title_view(&entities).unwrap();
        assert!(view.min_x < -3000.0 && view.max_x > -3000.0);
        assert!(view.min_y < -5000.0 && view.max_y > -5000.0);
        assert!(view.max_x < 9000.0);
    }

    #[test]
    fn expands_bulge_arc() {
        let entity = RawEntity {
            kind: "LWPOLYLINE".to_owned(),
            pairs: vec![
                Pair {
                    code: 10,
                    value: "0".to_owned(),
                },
                Pair {
                    code: 20,
                    value: "0".to_owned(),
                },
                Pair {
                    code: 42,
                    value: "1".to_owned(),
                },
                Pair {
                    code: 10,
                    value: "10".to_owned(),
                },
                Pair {
                    code: 20,
                    value: "0".to_owned(),
                },
            ],
            children: Vec::new(),
        };
        let points = bulged_polyline(&lwpolyline_vertices(&entity), false);
        assert!(points.len() > 10);
    }
}
