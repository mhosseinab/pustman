use crate::ItemId;
use std::str::FromStr;

#[test]
fn item_id_roundtrips_through_json() {
    let id = ItemId::new();
    let json = serde_json::to_string(&id).unwrap();
    assert_eq!(
        json,
        format!("\"{id}\""),
        "ItemId must serialize as its string form"
    );
    let restored: ItemId = serde_json::from_slice(json.as_bytes()).unwrap();
    assert_eq!(restored, id);
}

#[test]
fn item_id_rejects_invalid_string() {
    assert!(ItemId::from_str("not-a-ulid").is_err());
    assert!(serde_json::from_slice::<ItemId>(br#""not-a-ulid""#).is_err());
}
