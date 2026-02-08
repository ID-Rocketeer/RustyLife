use rustylife_core::Request;

#[test]
fn test_request_serialization() {
    let start = Request::Start;
    let json = serde_json::to_string(&start).unwrap();
    println!("Start JSON: {}", json);

    // Ensure we can deserialize it back
    let deserialized: Request = serde_json::from_str(&json).unwrap();
    assert_eq!(start, deserialized);

    let seed = Request::Seed("glider".to_string());
    let json_seed = serde_json::to_string(&seed).unwrap();
    println!("Seed JSON: {}", json_seed);
    let deserialized_seed: Request = serde_json::from_str(&json_seed).unwrap();
    assert_eq!(seed, deserialized_seed);
}
