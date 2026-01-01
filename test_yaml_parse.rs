use serde_yaml;

#[test]
fn test_parse_run_with_boolean() {
    let yaml = r#"
  setup-task:
    run: true
"#;

    let result: Result<serde_yaml::Value, _> = serde_yaml::from_str(yaml);
    println!("Parsed as Value: {:?}", result.unwrap());

    let yaml_struct = r#"
services:
  setup-task:
    run: true
"#;

    let config: Result<serde_yaml::Mapping, _> = serde_yaml::from_str(yaml_struct);
    println!("Parsed as Mapping: {:?}", config);
}
