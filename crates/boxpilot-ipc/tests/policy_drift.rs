use boxpilot_ipc::HelperMethod;
use std::collections::HashSet;

#[test]
fn every_helper_method_has_a_polkit_action_id_in_the_xml() {
    let xml = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../packaging/linux/polkit-1/actions/app.boxpilot.helper.policy"
    ))
    .expect("read policy XML");

    let mut declared: HashSet<String> = HashSet::new();
    for line in xml.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("<action id=\"") {
            if let Some(end) = rest.find('"') {
                declared.insert(rest[..end].to_string());
            }
        }
    }

    for m in HelperMethod::ALL {
        let id = m.polkit_action_id();
        assert!(
            declared.contains(&id),
            "polkit policy XML is missing action {id} (HelperMethod::{m:?})"
        );
    }
    assert_eq!(
        declared.len(),
        HelperMethod::ALL.len(),
        "extra action IDs in policy XML"
    );
}
