use crate::JoshResult;
use regex::Regex;
use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt::Write;

pub fn transform_with_template<F>(
    re: &Regex,
    template: &str,
    input: &str,
    globals: F,
) -> JoshResult<String>
where
    F: Fn(&str) -> Option<String>,
{
    let first_error: RefCell<Option<crate::JoshError>> = RefCell::new(None);

    let result = re
        .replace_all(input, |caps: &regex::Captures| {
            // Collect all named capture values
            let mut string_storage: HashMap<String, String> = HashMap::new();
            for name in re.capture_names().flatten() {
                if let Some(m) = caps.name(name) {
                    string_storage.insert(name.to_string(), m.as_str().to_string());
                }
            }

            // Use strfmt_map which calls our function for each key it needs
            match strfmt::strfmt_map(
                template,
                |mut fmt: strfmt::Formatter| -> Result<(), strfmt::FmtError> {
                    let key = fmt.key;

                    // First check named captures (higher priority)
                    if let Some(value) = string_storage.get(key) {
                        write!(fmt, "{}", value).map_err(|_| {
                            strfmt::FmtError::Invalid(format!(
                                "failed to write value for key: {}",
                                key
                            ))
                        })?;
                        return Ok(());
                    }

                    // Then call globals function (lower priority)
                    if let Some(global_value) = globals(key) {
                        write!(fmt, "{}", global_value).map_err(|_| {
                            strfmt::FmtError::Invalid(format!(
                                "failed to write global value for key: {}",
                                key
                            ))
                        })?;
                        return Ok(());
                    }

                    // Key not found - skip it (strfmt will leave the placeholder)
                    fmt.skip()
                },
            ) {
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
