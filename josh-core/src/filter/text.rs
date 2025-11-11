use crate::JoshResult;
use regex::Regex;
use std::cell::RefCell;
use std::collections::HashMap;

pub fn transform_with_template(
    re: &Regex,
    template: &str,
    input: &str,
    globals: &HashMap<String, String>,
) -> JoshResult<String> {
    let first_error: RefCell<Option<crate::JoshError>> = RefCell::new(None);

    let result = re
        .replace_all(input, |caps: &regex::Captures| {
            // Build a HashMap with all named captures and globals
            // We need to store the string values to keep them alive for the HashMap references
            let mut string_storage: HashMap<String, String> = HashMap::new();

            // Collect all named capture values
            for name in re.capture_names().flatten() {
                if let Some(m) = caps.name(name) {
                    string_storage.insert(name.to_string(), m.as_str().to_string());
                }
            }

            // Build the HashMap for strfmt with references to the stored strings
            let mut vars: HashMap<String, &dyn strfmt::DisplayStr> = HashMap::new();

            // Add all globals first (lower priority)
            for (key, value) in globals {
                vars.insert(key.clone(), value as &dyn strfmt::DisplayStr);
            }

            // Add all named captures (higher priority - will overwrite globals if there's a conflict)
            for (key, value) in &string_storage {
                vars.insert(key.clone(), value as &dyn strfmt::DisplayStr);
            }

            // Format the template, propagating errors
            match strfmt::strfmt(template, &vars) {
                Ok(s) => s,
                Err(e) => {
                    let mut error = first_error.borrow_mut();
                    if error.is_none() {
                        *error = Some(e.into());
                    }
                    caps[0].to_string()
                }
            }
        })
        .into_owned();

    match first_error.into_inner() {
        Some(e) => Err(e),
        None => Ok(result),
    }
}
