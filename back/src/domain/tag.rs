pub fn normalize_tag_path(input: &str) -> String {
    let trimmed = input.trim().trim_start_matches('/');
    trimmed.replace('/', ".")
}

pub fn shared_tag_path(sender_username: &str, sender_instance: &str, tag_path: &str) -> String {
    let label = sanitize_label(&format!("{}@{}", sender_username, sender_instance));
    let normalized = normalize_tag_path(tag_path);
    if normalized.is_empty() {
        format!("SharedToMe.{}", label)
    } else {
        format!("SharedToMe.{}.{}", label, normalized)
    }
}

fn sanitize_label(input: &str) -> String {
    input
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}
