#[derive(Debug, Clone)]
pub struct BackendMapping {
    pub username: String,
    pub instance_domain: String,
    pub backend_domain: String,
}
