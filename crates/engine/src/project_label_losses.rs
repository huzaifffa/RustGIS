//! Count label-setting losses while skipping feature data and retaining no moved-label arrays.

use serde::Deserialize;

use crate::Project;

#[derive(Default)]
struct MoveCount(usize);

impl<'de> Deserialize<'de> for MoveCount {
    /// Counts raw records without retaining their values, including damaged non-array settings.
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct Count;
        impl<'de> serde::de::Visitor<'de> for Count {
            type Value = MoveCount;
            /// Describes the optional project setting consumed by this visitor.
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("moved-label settings")
            }
            /// Counts entries using IgnoredAny so even an oversized list has bounded memory.
            fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                let mut count = 0usize;
                while seq.next_element::<serde::de::IgnoredAny>()?.is_some() {
                    count = count.saturating_add(1);
                }
                Ok(MoveCount(count))
            }
            /// Skips a damaged object and counts its discarded setting once.
            fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                while map.next_entry::<serde::de::IgnoredAny, serde::de::IgnoredAny>()?.is_some() {}
                Ok(MoveCount(1))
            }
            /// A boolean is one discarded move setting.
            fn visit_bool<E: serde::de::Error>(self, _: bool) -> Result<Self::Value, E> {
                Ok(MoveCount(1))
            }
            /// A signed number is one discarded move setting.
            fn visit_i64<E: serde::de::Error>(self, _: i64) -> Result<Self::Value, E> {
                Ok(MoveCount(1))
            }
            /// An unsigned number is one discarded move setting.
            fn visit_u64<E: serde::de::Error>(self, _: u64) -> Result<Self::Value, E> {
                Ok(MoveCount(1))
            }
            /// A fractional number is one discarded move setting.
            fn visit_f64<E: serde::de::Error>(self, _: f64) -> Result<Self::Value, E> {
                Ok(MoveCount(1))
            }
            /// Text is one discarded move setting.
            fn visit_str<E: serde::de::Error>(self, _: &str) -> Result<Self::Value, E> {
                Ok(MoveCount(1))
            }
            /// Null expresses no moved-label records and causes no loss notice.
            fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
                Ok(MoveCount(0))
            }
        }
        d.deserialize_any(Count)
    }
}

#[derive(Default, Deserialize)]
struct Class {
    #[serde(default)]
    moved: MoveCount,
    #[serde(default)]
    callout: Option<serde_json::Value>,
}

#[derive(Default, Deserialize)]
struct Labels {
    #[serde(default)]
    moved: MoveCount,
    #[serde(default)]
    callout: Option<serde_json::Value>,
    #[serde(default)]
    classes: Vec<Class>,
}

#[derive(Deserialize)]
struct Layer {
    #[serde(default)]
    labels: Labels,
}

#[derive(Default, Deserialize)]
struct Map {
    #[serde(default)]
    layers: Vec<Layer>,
}

#[derive(Deserialize)]
struct Offline {
    data: Map,
}

#[derive(Deserialize)]
struct File {
    maps: Vec<Map>,
    #[serde(default)]
    offline_areas: Vec<Offline>,
    #[serde(default)]
    styles: Vec<Layer>,
}

/// Zero and null explicitly disable callouts; other lengths must fit the supported range.
fn adjusted_callout(value: Option<&serde_json::Value>) -> bool {
    value.is_some_and(|v| v.as_f64().is_none_or(|n| n != 0.0 && !rustgis_map::symbology::CALLOUT_REACH.contains(&n)))
}

/// Compare raw record counts with retained state, including offline snapshots; one notice per kind.
pub(super) fn notices(text: &str, project: &Project) -> Result<Vec<String>, String> {
    let file: File = serde_json::from_str(text).map_err(|e| format!("could not read project label settings: {e}"))?;
    let (mut moves, mut callouts) = (0usize, 0usize);
    for layer in file.maps.iter().chain(file.offline_areas.iter().map(|a| &a.data)).flat_map(|m| &m.layers).chain(file.styles.iter()) {
        let labels = &layer.labels;
        moves = moves.saturating_add(labels.moved.0);
        callouts = callouts.saturating_add(usize::from(adjusted_callout(labels.callout.as_ref())));
        for class in &labels.classes {
            moves = moves.saturating_add(class.moved.0);
            callouts = callouts.saturating_add(usize::from(adjusted_callout(class.callout.as_ref())));
        }
    }
    let retained = project
        .maps
        .iter()
        .chain(project.offline_areas.iter().map(|a| &a.data))
        .flat_map(|m| &m.layers)
        .map(|l| &l.labels)
        .chain(project.styles.iter().map(|s| &s.labels))
        .flat_map(|labels| labels.in_order())
        .map(|c| c.moved.len())
        .fold(0usize, usize::saturating_add);
    let dropped = moves.saturating_sub(retained);
    let mut messages = Vec::new();
    if dropped > 0 {
        messages.push(format!(
            "{}: {dropped} invalid, duplicate or excess moved-label entry/setting(s) ignored while opening the project",
            project.name
        ));
    }
    if callouts > 0 {
        messages.push(format!("{}: {callouts} callout length setting(s) adjusted or disabled while opening the project", project.name));
    }
    Ok(messages)
}
