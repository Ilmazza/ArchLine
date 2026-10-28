//! Area and length fields for the objects the codec does not measure.
//!
//! The codec's `Object` field evaluates Area and Length for circles, arcs and
//! lines only. For anything else it gives up, and a field that cannot be
//! evaluated keeps its cached text, so the figure in a drawing's title block or
//! schedule stayed at the value of the last time AutoCAD saved it, with no
//! warning. This module measures the rest here (polylines, hatches, ellipses
//! and splines, with the AREA command's own geometry) and hands the number back
//! to the codec as a formula, so the picture (`\f "%lu2%pr2%ct8[0.01]"`) is
//! formatted by the same code as every other field.
//!
//! It is consulted only after the codec answered `None`; what the codec does
//! evaluate is never touched.

use codec::entities::table::CellValue;
use codec::fields::FieldContext;
use codec::objects::ObjectType;
use codec::{Document, EntityType, Handle};

/// The value of `prop` (`Area`, `Length`, `Perimeter`) of `entity`, for the
/// kinds of object the codec leaves out. `None` for any other combination.
fn measured_value(entity: &EntityType, prop: &str) -> Option<f64> {
    let area = match prop {
        "Area" => true,
        "Length" | "Perimeter" => false,
        _ => return None,
    };
    let value = match entity {
        // A hatch covers an area; it has no length of its own.
        EntityType::Hatch(hatch) => area.then(|| crate::entities::hatch::boundary_area(hatch)),
        EntityType::LwPolyline(_)
        | EntityType::Polyline(_)
        | EntityType::Polyline2D(_)
        | EntityType::Polyline3D(_)
        | EntityType::Ellipse(_)
        | EntityType::Spline(_) => {
            let (area_value, length) = crate::modules::draw::inquiry::area::measure(entity)?;
            if area { Some(area_value) } else { length }
        }
        _ => None,
    };
    value.filter(|v| v.is_finite())
}

/// What an `AcObjProp` code asks for.
struct Request<'a> {
    prop: &'a str,
    object: Handle,
    /// The `\f` picture exactly as written, quotes included.
    picture: Option<&'a str>,
}

fn between<'a>(text: &'a str, open: &str, close: &str) -> Option<&'a str> {
    let start = text.find(open)? + open.len();
    let end = text[start..].find(close)? + start;
    Some(&text[start..end])
}

fn parse<'a>(code: &'a str, objects: &[Handle]) -> Option<Request<'a>> {
    let code = code.trim();
    let code = code.strip_prefix("%<").and_then(|c| c.strip_suffix(">%")).unwrap_or(code).trim();
    if !code.starts_with("\\AcObjProp") {
        return None;
    }
    let prop = code.split(").").nth(1)?.split([' ', '\\']).next()?.trim();
    let object = match between(code, "_ObjIdx ", ">%") {
        Some(index) => *objects.get(index.trim().parse::<usize>().ok()?)?,
        None => Handle::new(between(code, "_ObjId ", ">%")?.trim().parse().ok()?),
    };
    let picture = code.find("\\f ").map(|at| code[at + 3..].trim());
    Some(Request { prop, object, picture })
}

/// The text of an `AcObjProp` code the codec could not evaluate.
pub(crate) fn evaluate(doc: &Document, code: &str, objects: &[Handle], ctx: &dyn FieldContext) -> Option<String> {
    let request = parse(code, objects)?;
    let value = measured_value(doc.get_entity(request.object)?, request.prop)?;
    let formula = match request.picture {
        Some(picture) => format!("\\AcExpr ({value:.15}) \\f {picture}"),
        None => format!("\\AcExpr ({value:.15})"),
    };
    codec::fields::evaluate_code(doc, &formula, &[], None, ctx)
}

/// Owner of an object handle (a field or a dictionary); `None` for an entity.
fn owner_of(doc: &Document, handle: Handle) -> Option<Handle> {
    if let Some(field) = doc.fields.get(&handle) {
        return Some(field.owner);
    }
    match doc.objects.get(&handle)? {
        ObjectType::Dictionary(dictionary) => Some(dictionary.owner),
        ObjectType::Unknown { owner, .. } => Some(*owner),
        _ => None,
    }
}

/// The entity a text container belongs to: where its chain of owners ends.
fn host_of(doc: &Document, container: Handle) -> Option<Handle> {
    let mut current = container;
    for _ in 0..12 {
        let owner = owner_of(doc, current)?;
        if owner_of(doc, owner).is_none() {
            return Some(owner);
        }
        current = owner;
    }
    None
}

/// A container's text with each `%<\_FldIdx N>%` replaced by the Nth shown value.
fn fill_template(template: &str, shown: &[String], in_mtext: bool) -> Option<String> {
    let mut out = String::new();
    let mut rest = template;
    while let Some(at) = rest.find("%<") {
        out.push_str(&rest[..at]);
        let after = &rest[at..];
        let Some(end) = after.find(">%") else {
            out.push_str(after);
            return Some(out);
        };
        let marker = &after[2..end];
        let index: usize = marker.contains("_FldIdx").then(|| marker.rsplit(' ').next()?.trim().parse().ok())??;
        let value = shown.get(index)?;
        out.push_str(&if in_mtext { value.replace('\\', "\\\\") } else { value.clone() });
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    Some(out)
}

/// The text a field-hosting entity shows.
fn text_slot(entity: &mut EntityType) -> Option<&mut String> {
    match entity {
        EntityType::MText(e) => Some(&mut e.value),
        EntityType::Text(e) => Some(&mut e.value),
        EntityType::AttributeDefinition(e) => Some(&mut e.default_value),
        EntityType::AttributeEntity(e) => Some(&mut e.value),
        _ => None,
    }
}

/// Puts `text` into the host, returning what it showed before.
fn set_host_text(doc: &mut Document, host: Handle, text: &str) -> Option<String> {
    if doc.get_entity(host).is_some() {
        return text_slot(doc.get_entity_mut(host)?).map(|slot| std::mem::replace(slot, text.to_string()));
    }
    // An attribute of a block reference is not an entity of its own.
    let insert = doc.entities().find_map(|entity| match entity {
        EntityType::Insert(i) if i.attributes.iter().any(|a| a.common.handle == host) => Some(i.common.handle),
        _ => None,
    })?;
    let EntityType::Insert(i) = doc.get_entity_mut(insert)? else {
        return None;
    };
    let attribute = i.attributes.iter_mut().find(|a| a.common.handle == host)?;
    Some(std::mem::replace(&mut attribute.value, text.to_string()))
}

/// What the file stores to tell a field text from one edited by hand.
fn text_checksum(text: &str) -> f64 {
    text.encode_utf16().enumerate().map(|(i, u)| (i as f64 + 1.0) * u as f64).sum()
}

/// After the codec has updated the fields an `event` calls for (1 open, 2 save,
/// 4 plot, 16 regen, 32 UPDATEFIELD), updates the ones it could not: a measured
/// Area or Length of a polyline, hatch, ellipse or spline. Same rules as the
/// codec: the field's own evaluation options pick it, and only a value that
/// changed is rewritten, in the field and in the host text. Returns the hosts
/// whose text changed.
pub(crate) fn update(doc: &mut Document, event: i32, only: Option<&[Handle]>, ctx: &dyn FieldContext) -> Vec<Handle> {
    struct Plan {
        host: Handle,
        container: Handle,
        text: String,
        values: Vec<(Handle, String)>,
    }
    let mut plans = Vec::new();
    for container in doc.fields.values().filter(|field| field.evaluator == "_text") {
        let Some(host) = host_of(doc, container.handle) else {
            continue;
        };
        if only.is_some_and(|only| !only.contains(&host)) {
            continue;
        }
        let mut kids: Vec<_> = doc.fields.values().filter(|field| field.owner == container.handle).collect();
        kids.sort_by_key(|field| u64::from(field.handle));
        let (mut shown, mut values) = (Vec::new(), Vec::new());
        for kid in kids {
            let Some(ObjectType::Field(stored)) = doc.objects.get(&kid.handle) else {
                shown.push(String::new());
                continue;
            };
            let cached = match stored.value.display() {
                "" => stored.value_string.clone(),
                text => text.to_string(),
            };
            let fresh = (stored.evaluation_option & event != 0)
                .then(|| evaluate(doc, &kid.code, &kid.objects, ctx))
                .flatten();
            match fresh {
                Some(text) if text != cached => {
                    values.push((kid.handle, text.clone()));
                    shown.push(text);
                }
                _ => shown.push(cached),
            }
        }
        if values.is_empty() {
            continue;
        }
        let in_mtext = matches!(doc.get_entity(host), Some(EntityType::MText(_)));
        if let Some(text) = fill_template(&container.code, &shown, in_mtext) {
            plans.push(Plan { host, container: container.handle, text, values });
        }
    }
    let mut changed = Vec::new();
    for plan in plans {
        for (kid, text) in &plan.values {
            if let Some(ObjectType::Field(field)) = doc.objects.get_mut(kid) {
                let mut value = CellValue::text(text);
                value.flags = 4;
                value.format = field.value.format.clone();
                field.value = value;
                field.value_string = text.clone();
                field.value_string_length = text.encode_utf16().count() as i32;
                field.evaluation_status = 2;
            }
        }
        if let Some(ObjectType::Field(container)) = doc.objects.get_mut(&plan.container) {
            for child in &mut container.child_values {
                if child.key == "ACFD_FIELDTEXT_CHECKSUM" {
                    child.value.numeric_value = text_checksum(&plan.text);
                }
            }
        }
        if set_host_text(doc, plan.host, &plan.text).is_some_and(|old| old != plan.text) {
            changed.push(plan.host);
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use crate::entities::field::{evaluate, update_fields};
    use codec::entities::{
        BoundaryEdge, BoundaryPath, Ellipse, Hatch, Line, LwPolyline, MText, PolylineEdge, Polyline3D, Spline,
        Vertex3DPolyline,
    };
    use codec::types::{Vector2, Vector3};
    use codec::{Document, EntityType, Handle};

    const AREA: &str = "\\AcObjProp Object(%<\\_ObjIdx 0>%).Area";
    const LENGTH: &str = "\\AcObjProp Object(%<\\_ObjIdx 0>%).Length";

    fn with_picture(code: &str, picture: &str) -> String {
        format!("{code} \\f \"{picture}\"")
    }

    fn show(doc: &Document, code: &str, object: Handle) -> Option<String> {
        evaluate(doc, code, &[object], None)
    }

    fn rectangle(doc: &mut Document, width: f64, height: f64) -> Handle {
        let mut polyline = LwPolyline::from_points(vec![
            Vector2::new(0.0, 0.0),
            Vector2::new(width, 0.0),
            Vector2::new(width, height),
            Vector2::new(0.0, height),
        ]);
        polyline.is_closed = true;
        doc.add_entity(EntityType::LwPolyline(polyline)).unwrap()
    }

    #[test]
    fn a_closed_polyline_shows_its_area_and_perimeter() {
        let mut doc = Document::new();
        let rect = rectangle(&mut doc, 4.0, 3.0);
        assert_eq!(show(&doc, &with_picture(AREA, "%lu2%pr2"), rect).as_deref(), Some("12.00"));
        assert_eq!(show(&doc, &with_picture(LENGTH, "%lu2%pr2"), rect).as_deref(), Some("14.00"));
    }

    #[test]
    fn without_a_picture_the_value_has_six_decimals_like_every_other_property() {
        let mut doc = Document::new();
        let rect = rectangle(&mut doc, 4.0, 3.0);
        assert_eq!(show(&doc, AREA, rect).as_deref(), Some("12.000000"));
    }

    #[test]
    fn an_open_polyline_with_a_bulge_counts_its_arc() {
        let mut doc = Document::new();
        let mut polyline = LwPolyline::from_points(vec![Vector2::new(0.0, 0.0), Vector2::new(2.0, 0.0)]);
        polyline.vertices[0].bulge = 1.0;
        let semicircle = doc.add_entity(EntityType::LwPolyline(polyline)).unwrap();
        assert_eq!(show(&doc, &with_picture(LENGTH, "%lu2%pr2"), semicircle).as_deref(), Some("3.14"));
        assert_eq!(show(&doc, &with_picture(AREA, "%lu2%pr2"), semicircle).as_deref(), Some("1.57"));
    }

    #[test]
    fn a_hatch_shows_the_area_it_covers() {
        let mut doc = Document::new();
        let mut hatch = Hatch::new();
        let mut path = BoundaryPath::external();
        let corners = [(0.0, 0.0), (4.0, 0.0), (4.0, 3.0), (0.0, 3.0)];
        path.add_edge(BoundaryEdge::Polyline(PolylineEdge::new(
            corners.iter().map(|(x, y)| Vector2::new(*x, *y)).collect(),
            true,
        )));
        hatch.paths.push(path);
        let handle = doc.add_entity(EntityType::Hatch(Box::new(hatch))).unwrap();
        assert_eq!(show(&doc, &with_picture(AREA, "%lu2%pr2"), handle).as_deref(), Some("12.00"));
    }

    #[test]
    fn an_ellipse_shows_area_and_length() {
        let mut doc = Document::new();
        let ellipse = Ellipse::from_center_axes(Vector3::ZERO, Vector3::new(2.0, 0.0, 0.0), 0.5);
        let handle = doc.add_entity(EntityType::Ellipse(ellipse)).unwrap();
        assert_eq!(show(&doc, &with_picture(AREA, "%lu2%pr2"), handle).as_deref(), Some("6.28"));
        assert_eq!(show(&doc, &with_picture(LENGTH, "%lu2%pr2"), handle).as_deref(), Some("9.69"));
    }

    #[test]
    fn a_3d_polyline_shows_its_spatial_length() {
        let mut doc = Document::new();
        let mut polyline = Polyline3D::new();
        polyline.vertices = vec![Vertex3DPolyline::from_xyz(0.0, 0.0, 0.0), Vertex3DPolyline::from_xyz(3.0, 4.0, 12.0)];
        let handle = doc.add_entity(EntityType::Polyline3D(polyline)).unwrap();
        assert_eq!(show(&doc, &with_picture(LENGTH, "%lu2%pr2"), handle).as_deref(), Some("13.00"));
    }

    #[test]
    fn a_spline_shows_its_length() {
        let mut doc = Document::new();
        let spline = Spline::from_control_points(
            3,
            vec![Vector3::ZERO, Vector3::new(3.0, 0.0, 0.0), Vector3::new(7.0, 0.0, 0.0), Vector3::new(10.0, 0.0, 0.0)],
        );
        let handle = doc.add_entity(EntityType::Spline(spline)).unwrap();
        assert_eq!(show(&doc, &with_picture(LENGTH, "%lu2%pr2"), handle).as_deref(), Some("10.00"));
    }

    #[test]
    fn the_conversion_factor_of_the_picture_applies() {
        let mut doc = Document::new();
        let rect = rectangle(&mut doc, 4.0, 3.0);
        assert_eq!(show(&doc, &with_picture(AREA, "%lu2%pr4%ct8[0.01]"), rect).as_deref(), Some("0.1200"));
    }

    #[test]
    fn the_picture_the_dialog_writes_for_an_area_evaluates() {
        let mut doc = Document::new();
        let rect = rectangle(&mut doc, 4.0, 3.0);
        assert_eq!(show(&doc, &with_picture(AREA, "%lu2%pr2%ct8[0.01]%qf1"), rect).as_deref(), Some("0.12"));
    }

    #[test]
    fn a_property_the_object_does_not_have_stays_unevaluated() {
        let mut doc = Document::new();
        let line = doc
            .add_entity(EntityType::Line(Line::from_points(Vector3::ZERO, Vector3::new(1.0, 0.0, 0.0))))
            .unwrap();
        assert_eq!(show(&doc, AREA, line), None);
    }

    /// An MTEXT hosting the field, its cached text stale on purpose.
    fn host_with_field(doc: &mut Document, code: &str, object: Handle, cached: &str) -> Handle {
        let host = doc.add_entity(EntityType::MText(Box::new(MText::new()))).unwrap();
        crate::entities::field::set_text_field(doc, host, Some((code.to_string(), vec![object])), cached);
        host
    }

    fn shown_by(doc: &Document, host: Handle) -> String {
        match doc.get_entity(host) {
            Some(EntityType::MText(text)) => text.value.clone(),
            other => panic!("not a text: {other:?}"),
        }
    }

    #[test]
    fn updating_the_fields_follows_the_polyline_not_the_stored_text() {
        let mut doc = Document::new();
        let rect = rectangle(&mut doc, 4.0, 3.0);
        let host = host_with_field(&mut doc, &with_picture(AREA, "%lu2%pr2"), rect, "99.00");
        assert_eq!(shown_by(&doc, host), "99.00", "inserted with the stored value");
        let (changed, _) = update_fields(&mut doc, 32, None);
        assert_eq!(changed, vec![host]);
        assert_eq!(shown_by(&doc, host), "12.00");
        if let Some(EntityType::LwPolyline(polyline)) = doc.get_entity_mut(rect) {
            polyline.vertices[1].location.x = 5.0;
            polyline.vertices[2].location.x = 5.0;
        }
        let (changed, _) = update_fields(&mut doc, 32, None);
        assert_eq!(changed, vec![host]);
        assert_eq!(shown_by(&doc, host), "15.00");
        // Nothing moved since: nothing to rewrite.
        assert!(update_fields(&mut doc, 32, None).0.is_empty());
    }

    #[test]
    fn an_update_leaves_alone_a_host_it_was_not_asked_about() {
        let mut doc = Document::new();
        let rect = rectangle(&mut doc, 4.0, 3.0);
        let host = host_with_field(&mut doc, &with_picture(AREA, "%lu2%pr2"), rect, "99.00");
        let other = host_with_field(&mut doc, &with_picture(LENGTH, "%lu2%pr2"), rect, "98.00");
        update_fields(&mut doc, 32, Some(&[host]));
        assert_eq!(shown_by(&doc, host), "12.00");
        assert_eq!(shown_by(&doc, other), "98.00");
    }
}
