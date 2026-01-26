# Archypix Resolver

WebFinger resolver service that maps usernames to backend domains in the Archypix distributed architecture.

## Features

- **WebFinger Endpoint**: RFC 7033 compliant `/.well-known/webfinger` endpoint
- **In-Memory Cache**: Moka-based TTL cache for fast lookups
- **PostgreSQL Storage**: Authoritative mapping storage with sqlx
- **Admin API**: Token-protected endpoint for backends to register/update users
- **Health Check**: `/health` endpoint for monitoring

## Architecture

The resolver is the central discovery service that:
1. Receives WebFinger queries from clients/frontends
2. Returns the authoritative backend domain for a given username
3. Caches results in-memory with configurable TTL
4. Allows backends to register their users via authenticated API

## Configuration

Copy `.env.example` to `.env` and configure:

```bash
cp .env.example .env
```

### Environment Variables

- `DATABASE_URL`: PostgreSQL connection string (default: `postgres://archypix:archypix@localhost/archypix_resolver`)
- `MANAGED_DOMAIN`: The domain that the resolver manages. Corresponds to the part after the `:` in usernames.
- `ADMIN_TOKEN`: Secret token for update API (required in production)
- `LISTEN_ADDR`: Server bind address (default: `0.0.0.0:8080`)
- `CACHE_TTL_SECS`: Cache time-to-live in seconds (default: `3600`)
- `CACHE_MAX_CAPACITY`: Maximum cache entries (default: `100000`)
- `RUST_LOG`: Log level (default: `info,archypix_resolver=debug`)

## Database Setup

The service automatically creates the required table on startup:

```sql
CREATE TABLE IF NOT EXISTS user_mappings (
    username VARCHAR(255) PRIMARY KEY,
    backend_url VARCHAR(255) NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
```

## API Endpoints

### WebFinger Discovery

**GET** `/.well-known/webfinger?resource=acct:@username:domain`

Returns WebFinger response with links to the user's backend.

Example:
```bash
curl "http://localhost:8080/.well-known/webfinger?resource=acct:@alice:example.com"
```

Response:
```json
{
  "subject": "acct:alice@backend1.archypix.com",
  "links": [
    {
      "rel": "backend_url",
      "href": "https://backend1.archypix.com"
    }
  ]
}
```

### Update User Mapping

**POST** `/api/update`

Register or update a user mapping (requires admin token).

Example:
```bash
curl -X POST http://localhost:8080/api/update \
  -H "Content-Type: application/json" \
  -d '{
    "token": "your-admin-token",
    "username": "alice",
    "backend_url": "https://backend1.archypix.com"
  }'
```

Response:
```json
{
  "success": true,
  "message": "Mapping updated for user alice"
}
```

### Health Check

**GET** `/health`

Returns service health status.

Example:
```bash
curl http://localhost:8080/health
```

Response:
```json
{
  "status": "healthy",
  "service": "archypix-resolver"
}
```

## Running

### Development

```bash
cargo run
```

### Production

```bash
cargo build --release
./target/release/archypix-resolver
```

### Docker (future)

```bash
docker build -t archypix-resolver .
docker run -p 8080:8080 --env-file .env archypix-resolver
```

## Cache Behavior

- **Cache Hit**: Returns immediately from memory
- **Cache Miss**: Queries PostgreSQL, populates cache, returns result
- **Cache Update**: On successful mapping update via `/api/update`
- **Cache Expiry**: Automatic TTL-based eviction (configurable)

## Security Notes

1. **Change ADMIN_TOKEN**: Default token is for development only
2. **Use HTTPS**: Deploy behind reverse proxy with TLS
3. **Rate Limiting**: Consider adding rate limiting for production
4. **Network Security**: Restrict `/api/update` to backend network only

## Integration with Backends

Backends should call `/api/update` when:
- New user registers
- User migrates to different backend
- User account is deleted (update with tombstone or special domain)

Example backend integration:
```rust
async fn register_user_with_resolver(username: &str, backend_url: &str) -> Result<()> {
    let client = reqwest::Client::new();
    client.post("https://resolver.archypix.com/api/update")
        .json(&serde_json::json!({
            "token": env::var("RESOLVER_ADMIN_TOKEN")?,
            "username": username,
            "backend_url": backend_url
        }))
        .send()
        .await?;
    Ok(())
}
```

## Monitoring

- Check `/health` endpoint for liveness probes
- Monitor logs for cache hit/miss rates
- Track database connection pool metrics
- Alert on failed update attempts (unauthorized access)

## Performance

- Typical cache hit latency: < 1ms
- Cache miss (DB query): ~5-10ms
- Recommended cache TTL: 1-6 hours depending on user migration frequency
- Supports 100k+ cached users by default

## License

Part of the Archypix project.
