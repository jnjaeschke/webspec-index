//! The semantics package embedded in every build: `(path, YAML)` pairs in load
//! order. `effects/` feeds the effects catalog, `state/` the state catalog.

macro_rules! semantics_files {
    ($($path:literal),* $(,)?) => {
        &[$(($path, include_str!(concat!("../../data/semantics/", $path)))),*]
    };
}

pub(crate) const FILES: &[(&str, &str)] = semantics_files![
    "effects/00-vocabulary.yaml",
    "effects/10-events.yaml",
    "effects/20-scheduling.yaml",
    "effects/30-script-context.yaml",
    "effects/40-reviewed.yaml",
    "effects/50-host-implementations.yaml",
    "state/00-types.yaml",
    "state/10-fields.yaml",
    "state/20-rules.yaml",
];

#[cfg(test)]
mod tests {
    use super::FILES;

    #[test]
    fn bundled_files_list_every_package_file() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("data/semantics");
        let mut on_disk = Vec::new();
        for dir in ["effects", "state"] {
            for entry in std::fs::read_dir(root.join(dir)).unwrap() {
                let name = entry.unwrap().file_name().into_string().unwrap();
                on_disk.push(format!("{dir}/{name}"));
            }
        }
        on_disk.sort();
        let bundled = FILES
            .iter()
            .map(|(path, _)| path.to_string())
            .collect::<Vec<_>>();
        assert_eq!(bundled, on_disk);
    }
}
