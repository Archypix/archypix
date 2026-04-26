# Backend Technology Considerations

## Framework Choice: Axum
- Already proven in resolver component
- Excellent async/await support with Tokio
- Robust routing, middleware (via Tower), and error handling
- Consistent codebase across resolver and backend
- Good performance and scalability for microservices

## Database Access: SQLx
- Excellent PostgreSQL feature support (LTREE, JSONB, custom types, etc.)
- Compile-time checked SQL with macros
- Direct SQL control for performance optimization
- Team familiarity from resolver implementation
- Migration capabilities already in use
- Reduced abstraction overhead compared to ORMs

## Architecture Approach
- Layered separation of concerns
- Shared application state pattern (similar to resolver)
- Modular organization by domain/business capability
- Placeholder implementations for complex components
- Designed for evolution as schema and requirements change
