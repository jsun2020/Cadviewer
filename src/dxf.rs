use std::collections::{HashMap, HashSet};
use std::f64::consts::{PI, TAU};

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
    },
    Text {
        x: f64,
        y: f64,
        height: f64,
        rotation: f64,
        value: String,
    },
    Point {
        x: f64,
        y: f64,
    },
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

pub fn parse(input: &str) -> Result<Scene, String> {
    let pairs = parse_pairs(input)?;
    let mut blocks = HashMap::new();
    let mut entities = Vec::new();
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
            }
        }
        index += 1;
    }

    let mut primitives = Vec::new();
    let mut active_blocks = HashSet::new();
    emit_entities(
        &entities,
        &blocks,
        Affine::IDENTITY,
        &mut active_blocks,
        0,
        &mut primitives,
    );

    let mut bounds = Bounds::empty();
    for primitive in &primitives {
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
                value,
                ..
            } => {
                bounds.add(Point::new(*x, *y));
                bounds.add(Point::new(
                    *x + height.abs() * value.chars().count() as f64 * 0.65,
                    *y + height.abs(),
                ));
            }
            Primitive::Point { x, y } => bounds.add(Point::new(*x, *y)),
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
        svg.push_str(r#"<rect width="100%" height="100%" fill="white"/>"#);
        svg.push('\n');
        svg.push_str(&format!(
            r##"<g fill="none" stroke="#111827" stroke-width="{stroke_width}" stroke-linecap="round" stroke-linejoin="round">"##
        ));
        svg.push('\n');

        for primitive in &self.primitives {
            match primitive {
                Primitive::Polyline {
                    points,
                    closed,
                    filled,
                } if points.len() >= 2 => {
                    let tag = if *closed { "polygon" } else { "polyline" };
                    svg.push('<');
                    svg.push_str(tag);
                    svg.push_str(r#" points=""#);
                    for &(x, y) in points {
                        let (x, y) = map(x, y);
                        svg.push_str(&format!("{x:.4},{y:.4} "));
                    }
                    svg.push('"');
                    if *filled {
                        svg.push_str(r##" fill="#d1d5db""##);
                    }
                    svg.push_str("/>\n");
                }
                Primitive::Point { x, y } => {
                    let (x, y) = map(*x, *y);
                    svg.push_str(&format!(
                        r#"<circle cx="{x:.4}" cy="{y:.4}" r="{point_radius}"/>"#
                    ));
                    svg.push('\n');
                }
                _ => {}
            }
        }
        svg.push_str("</g>\n");

        for primitive in &self.primitives {
            if let Primitive::Text {
                x,
                y,
                height: text_height,
                rotation,
                value,
            } = primitive
            {
                let (x, y) = map(*x, *y);
                let font_size = (text_height.abs() * scale).max(2.0);
                let escaped = escape_xml(&plain_text(value));
                svg.push_str(&format!(
                    r##"<text x="{x:.4}" y="{y:.4}" font-family="Arial, sans-serif" font-size="{font_size:.4}" fill="#111827" transform="rotate({:.4} {x:.4} {y:.4})">{escaped}</text>"##,
                    -*rotation
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

fn emit_entities(
    entities: &[RawEntity],
    blocks: &HashMap<String, Block>,
    transform: Affine,
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
        emit_entity(entity, blocks, transform, active_blocks, depth, output);
    }
}

fn emit_entity(
    entity: &RawEntity,
    blocks: &HashMap<String, Block>,
    transform: Affine,
    active_blocks: &mut HashSet<String>,
    depth: usize,
    output: &mut Vec<Primitive>,
) {
    match entity.kind.as_str() {
        "LINE" => push_polyline(
            output,
            vec![
                transform.apply(entity.point(10, 20)),
                transform.apply(entity.point(11, 21)),
            ],
            false,
            false,
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
            );
        }
        "LWPOLYLINE" => {
            let vertices = lwpolyline_vertices(entity);
            let closed = entity.integer(70, 0) & 1 != 0;
            let points = bulged_polyline(&vertices, closed)
                .into_iter()
                .map(|point| transform.apply(point))
                .collect();
            push_polyline(output, points, closed, false);
        }
        "POLYLINE" => {
            let points: Vec<_> = entity
                .children
                .iter()
                .filter(|vertex| vertex.integer(70, 0) & 128 == 0)
                .map(|vertex| transform.apply(vertex.point(10, 20)))
                .collect();
            push_polyline(output, points, entity.integer(70, 0) & 1 != 0, false);
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
            );
        }
        "SOLID" | "TRACE" | "3DFACE" => {
            let points = [10, 11, 13, 12]
                .into_iter()
                .map(|code| transform.apply(entity.point(code, code + 10)))
                .collect();
            push_polyline(output, points, true, entity.kind != "3DFACE");
        }
        "POINT" => {
            let point = transform.apply(entity.point(10, 20));
            output.push(Primitive::Point {
                x: point.x,
                y: point.y,
            });
        }
        "TEXT" | "ATTRIB" | "ATTDEF" => emit_text(
            output,
            transform,
            entity.point(10, 20),
            entity.number(40, 1.0),
            entity.number(50, 0.0),
            entity.text(1).unwrap_or_default(),
        ),
        "MTEXT" => {
            let mut value = String::new();
            for pair in &entity.pairs {
                if pair.code == 3 || pair.code == 1 {
                    value.push_str(&pair.value);
                }
            }
            emit_text(
                output,
                transform,
                entity.point(10, 20),
                entity.number(40, 1.0),
                entity.number(50, 0.0),
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
            );
        }
        "INSERT" => emit_insert(entity, blocks, transform, active_blocks, depth, output),
        "DIMENSION" => {
            if let Some(name) = entity.text(2) {
                emit_block(name, blocks, transform, active_blocks, depth + 1, output);
            }
        }
        _ => {}
    }
}

fn emit_text(
    output: &mut Vec<Primitive>,
    transform: Affine,
    location: Point,
    height: f64,
    rotation: f64,
    value: &str,
) {
    let point = transform.apply(location);
    output.push(Primitive::Text {
        x: point.x,
        y: point.y,
        height: height * transform.average_scale(),
        rotation: rotation + transform.x_axis_angle(),
        value: value.to_owned(),
    });
}

fn emit_insert(
    entity: &RawEntity,
    blocks: &HashMap<String, Block>,
    parent: Affine,
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
                parent.then(local),
                active_blocks,
                depth + 1,
                output,
            );
        }
    }
}

fn emit_block(
    name: &str,
    blocks: &HashMap<String, Block>,
    transform: Affine,
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
        transform,
        active_blocks,
        depth,
        output,
    );
    active_blocks.remove(&key);
}

fn push_polyline(output: &mut Vec<Primitive>, points: Vec<Point>, closed: bool, filled: bool) {
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
    value
        .replace("\\P", "\n")
        .replace("%%d", "°")
        .replace("%%p", "±")
        .replace("%%c", "⌀")
        .replace("\\~", " ")
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
        assert!(svg.contains("<polyline"));
        assert!(svg.contains("viewBox=\"0 0"));
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
