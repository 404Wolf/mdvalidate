use mdvalidate::{mdschema::validator::errors::pretty_print_error, Validator};

fn main() {
    // Define a simple schema: a heading with a name and a list
    let schema = r#"
# `name:/[A-Za-z]+/`'s grocery list!

## Items

- `fruit:/[A-Za-z]+/`{,}
"#;

    // Input markdown that matches the schema
    let input = r#"
# Wolf's grocery list!

## Items

- Apple
- Banana
"#;

    // Create a validator and validate the input
    let mut validator = Validator::new_complete(schema, input).expect("Failed to create validator");
    validator.validate();

    // Check for errors
    let (errors, matches) = validator.report();

    let error_vec: Vec<_> = errors.collect();

    if error_vec.is_empty() {
        println!("✓ Validation successful!");
        println!("Matches: {:?}", matches);
    } else {
        println!("✗ Validation failed with {} error(s):", error_vec.len());
        for error in error_vec {
            let pretty_print = pretty_print_error(error, &validator, "example.md")
                .expect("Failed to pretty print error");
            println!("{}", pretty_print);
        }
    }
}
