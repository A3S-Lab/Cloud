pub(super) fn validate_form_identity_text(name: &str, description: &str) -> Result<(), String> {
    validate_text("Form name", name, 1, 120)?;
    validate_text("Form description", description, 0, 4_096)
}

fn validate_text(label: &str, value: &str, minimum: usize, maximum: usize) -> Result<(), String> {
    let trimmed_length = value.trim().chars().count();
    let stored_length = value.chars().count();
    if trimmed_length < minimum || stored_length > maximum || value.contains('\0') {
        return Err(format!(
            "{label} must contain between {minimum} and {maximum} characters"
        ));
    }
    Ok(())
}
