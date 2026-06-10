# Frontend Architecture (MVP)

## 1. Goals and constraints

The frontend serves as both the PoC validation tool and the shipped MVP interface. Key constraints:

- **AI-agent-friendly codebase** — component source lives in the repo, not buried in `node_modules`. Agents can grep, read, and edit every
  component file directly.
- **Minimal boilerplate** — no SSR, no build-time data fetching, no framework-level opinions beyond routing. The architecture doc specifies a
  "static CDN" deployment; the frontend is a pure SPA.
- **Complete design system** — Archypix requires complex UI: hierarchical tag pickers, nested date-range pickers for segmentation, pipeline
  configuration forms, virtualized photo grids.
- **MVP-quality UX** — not a throwaway wireframe. Visual design, loading states, error handling, and responsive layout should be close to the
  final product.

---

## 2. Technology stack

| Concern        | Choice                          | Rationale                                                                                                          |
|----------------|---------------------------------|--------------------------------------------------------------------------------------------------------------------|
| UI framework   | **React 19 + TypeScript**       | Largest AI training corpus, mature ecosystem, no overhead                                                          |
| Bundler        | **Vite**                        | Near-zero config, fast HMR, static output matches "static CDN" deployment model                                    |
| Routing        | **React Router v7**             | Simple file-based-ish routing, no SSR complexity                                                                   |
| Server state   | **TanStack Query v5**           | Handles caching, pagination, background refetch, optimistic updates — eliminates most manual fetch code            |
| Client state   | **Zustand**                     | Minimal store for auth session and UI-only state (sidebar open, selected photos)                                   |
| Design system  | **shadcn/ui + Tailwind CSS v4** | Component source copied into repo → fully editable; Radix UI primitives ensure accessibility and complex behaviors |
| Forms          | **React Hook Form + Zod**       | Uncontrolled forms with schema validation; Zod schemas serve as API contract documentation                         |
| Date pickers   | **react-day-picker v9**         | Already integrated with shadcn/ui Calendar; supports range and multi-month modes                                   |
| Virtualization | **@tanstack/virtual**           | Virtualized photo grid for large collections                                                                       |
| Drag-and-drop  | **@dnd-kit**                    | Reordering pipeline services within the tagging pipeline editor                                                    |
| HTTP client    | **axios**                       | Interceptor for JWT attach + auto-refresh; consistent error shape                                                  |

### Why shadcn/ui over Mantine or Ant Design

shadcn/ui runs `npx shadcn add <component>` which copies the component's TypeScript source directly into `src/components/ui/`. The file is then
a plain project file — agents can read it with `Read`, grep it, and edit it like any other source file. Mantine and Ant Design components live in
`node_modules` and cannot be directly modified without forking. For Archypix's custom components (hierarchical tag picker, pipeline editor), this
matters significantly.

### Why not Next.js

The backend architecture spec §5.1 explicitly places the frontend as a "static CDN". SSR/ISR provides no benefit here and adds significant
framework ceremony that impedes AI-agent development.

---

## 3. Project structure

```
front/
├── src/
│   ├── api/               # Typed axios wrappers, one file per domain
│   │   ├── client.ts      # Axios instance + JWT interceptor + refresh logic
│   │   ├── pictures.ts
│   │   ├── tags.ts
│   │   ├── tagging.ts
│   │   ├── shares.ts
│   │   ├── auth.ts
│   │   └── settings.ts
│   ├── components/
│   │   ├── ui/            # shadcn/ui copied components (auto-managed by shadcn CLI)
│   │   ├── photos/        # PhotoGrid, PhotoCard, PhotoDetail, UploadDropzone
│   │   ├── tags/          # TagPicker, TagTree, TagBadge, TagBreadcrumb
│   │   ├── tagging/       # PipelineList, ServiceCard, RuleEditor, SegmentEditor
│   │   ├── shares/        # ShareList, IncomingShareCard, OutgoingShareCard
│   │   └── layout/        # AppShell, Sidebar, TopBar, Breadcrumb
│   ├── hooks/             # Custom hooks wrapping TanStack Query calls
│   │   ├── usePictures.ts
│   │   ├── useTags.ts
│   │   ├── useTaggingServices.ts
│   │   └── useShares.ts
│   ├── stores/
│   │   ├── auth.ts        # Zustand: current user, JWT, refresh token
│   │   └── selection.ts   # Zustand: multi-photo selection set
│   ├── pages/             # Route-level components
│   │   ├── LoginPage.tsx
│   │   ├── GalleryPage.tsx
│   │   ├── PhotoPage.tsx
│   │   ├── TagsPage.tsx
│   │   ├── TaggingPage.tsx
│   │   ├── SharesPage.tsx
│   │   ├── SettingsPage.tsx
│   │   ├── TrashPage.tsx
│   │   └── AdminPage.tsx
│   ├── lib/
│   │   ├── schemas.ts     # Zod schemas mirroring API request/response shapes
│   │   ├── utils.ts       # cn(), date formatting, tag path helpers
│   │   └── constants.ts
│   ├── App.tsx            # Router setup
│   └── main.tsx
├── public/
├── index.html
├── vite.config.ts
├── tailwind.config.ts
├── tsconfig.json
└── package.json
```

---

## 4. Routes

| Path           | Page              | Auth           |
|----------------|-------------------|----------------|
| `/login`       | LoginPage         | Public         |
| `/register`    | RegisterPage      | Public         |
| `/`            | GalleryPage       | Required       |
| `/photos/:id`  | PhotoPage         | Required       |
| `/tags`        | TagsPage          | Required       |
| `/tagging`     | TaggingPage       | Required       |
| `/tagging/:id` | ServiceEditorPage | Required       |
| `/shares`      | SharesPage        | Required       |
| `/settings`    | SettingsPage      | Required       |
| `/trash`       | TrashPage         | Required       |
| `/admin`       | AdminPage         | Admin JWT only |

A `<ProtectedRoute>` wrapper component checks `authStore.user` and redirects to `/login` if absent. Admin routes additionally check `user.is_admin`.

---

## 5. Authentication flow

The backend issues a short-lived JWT (`/api/auth/login`) and a refresh token. The frontend stores both in `localStorage` (acceptable for an MVP;
httpOnly cookies are the v2 hardening step).

`src/api/client.ts` sets up two Axios interceptors:

1. **Request interceptor** — attaches `Authorization: Bearer <access_token>` to every request.
2. **Response interceptor** — on 401, calls `/api/auth/refresh` once, updates the stored token, and retries the original request. If refresh also
   fails, clears auth state and redirects to `/login`.

Zustand `authStore` holds `{ user, accessToken, refreshToken }`. It is initialized at app boot by reading `localStorage`, then kept in sync by
the interceptor.

---

## 6. Server state with TanStack Query

All API reads go through TanStack Query. Query keys follow the pattern `['domain', 'list'|'detail', ...params]`:

```ts
// hooks/usePictures.ts
export function usePictures(filters: PictureFilters) {
    return useInfiniteQuery({
        queryKey: ['pictures', filters],
        queryFn: ({pageParam = 1}) => api.pictures.list({...filters, page: pageParam}),
        getNextPageParam: (last) => last.has_next ? last.page + 1 : undefined,
    });
}

export function useAddTag() {
    const qc = useQueryClient();
    return useMutation({
        mutationFn: api.tags.batchEdit,
        onSuccess: (_, vars) => {
            qc.invalidateQueries({queryKey: ['pictures']});
            qc.invalidateQueries({queryKey: ['tags', 'detail', vars.picture_ids[0]]});
        },
    });
}
```

Mutations always invalidate the relevant query keys on success. There is no manual cache management.

---

## 7. Design system

### Base

shadcn/ui with the **zinc** neutral palette and a **sky** primary accent. Photos look best on a neutral dark background; the default color mode
is dark, with a light mode toggle persisted to `localStorage`.

Components added via `npx shadcn add`:

- `button`, `input`, `label`, `textarea`, `select`, `checkbox`, `switch`, `radio-group`
- `dropdown-menu`, `context-menu`, `popover`, `tooltip`, `dialog`, `alert-dialog`, `sheet`
- `command` (cmdk) — base for `TagPicker`
- `calendar` (react-day-picker) — base for `DateRangePicker`
- `card`, `badge`, `separator`, `scroll-area`, `skeleton`
- `table`, `tabs`, `accordion`
- `sonner` (toast notifications)
- `form` (React Hook Form integration)
- `avatar`, `breadcrumb`, `pagination`

### Custom components built on top

**`TagPicker`** (`components/tags/TagPicker.tsx`)

Built on shadcn `Command`. Displays an input that opens a popover with a filtered list of tag paths. Supports:

- Fuzzy search over all user tags (fetched via TanStack Query, cached)
- Keyboard navigation and selection
- Multiple selection mode (for adding tags to pictures)
- Creation of new tag paths (any valid `[A-Za-z0-9_/]` string)
- Visual display of the selected path as `TagBadge` chips

**`TagTree`** (`components/tags/TagTree.tsx`)

Recursive component that renders the tag hierarchy as a collapsible tree. Each node shows the tag label and a count badge. Used in the sidebar
filter panel and on `TagsPage`. Built on shadcn `Collapsible`.

**`DateRangePicker`** (`components/ui/DateRangePicker.tsx`)

Extends shadcn `Calendar` to expose `{ from: Date, to: Date }` ranges. Used in segmentation service forms and in the gallery date filter.
Supports month navigation, keyboard input, and optional presets (Last 7 days, Last month, Custom).

**`PhotoGrid`** (`components/photos/PhotoGrid.tsx`)

Virtualized masonry/grid layout using `@tanstack/virtual`. Each cell is a `PhotoCard` with:

- Thumbnail loaded via presigned URL (`/api/authenticated/pictures/{id}/url?variant=small`)
- BlurHash placeholder while loading (displayed via `blurhash` package)
- Selection checkbox (controlled by `selectionStore`)
- Tag badge strip on hover

**`PipelineServiceEditor`** (`components/tagging/`)

A three-panel layout: service list on the left, service header/settings in the center, rule/segment list on the right. Each service type renders
a different rule editor:

- **SharedTagMapping**: list of `(incoming_share, assign_tag)` pairs, `TagPicker` for tag selection
- **Rule**: predicate input (text field for MVP, structured builder in v2) + `TagPicker`
- **Segmentation**: nested `DateRangePicker` + tag assignment + `@dnd-kit` for reordering segments within a level

---

## 8. Pages

### GalleryPage

The main view. Top bar has a search/filter bar (tag filter, date range, owned/shared toggle). Main area is `PhotoGrid` with infinite scroll via
`useInfiniteQuery`. A collapsible left sidebar shows `TagTree` for filter navigation.

Floating action bar appears when photos are selected (multi-select via checkbox or Shift+click): shows "Add tag", "Remove tag", "Delete" actions.

### PhotoPage

Two-column layout. Left: full-size image (loads via `original` presigned URL). Right: metadata panel with tabs:

- **Tags** — `TagBadge` list, `TagPicker` to add manual tags, provenance toggle to show source per tag
- **Info** — EXIF data, file size, upload date, version history
- **Shares** — which outgoing shares cover this picture

### TaggingPage

Lists all tagging services in pipeline order. Each row shows service type, enabled state (toggle), `requires`/`excludes` tags as `TagBadge`
chips, and a rule count. Drag-and-drop reordering (pipeline order is meaningful for `requires`/`excludes` resolution). Click opens
`ServiceEditorPage`.

### SharesPage

Tabbed view: **Incoming** | **Outgoing**. Each card shows sender/recipient, shared tag path, status badge, and action buttons (Accept/Reject for
incoming; Revoke for outgoing). Accepts and rejects call the corresponding API endpoints and invalidate the shares query.

---

## 9. API client conventions

`src/api/client.ts` exports a typed `api` object:

```ts
export const api = {
    auth: {login, logout, refresh, me},
    pictures: {list, get, getUrl, beginUpload, completeUpload},
    tags: {list, listForPicture, batchEdit},
    tagging: {
        listServices, getService, createService, updateService, deleteService,
        addRule, deleteRule, addSegment, deleteSegment, addMapping, deleteMapping
    },
    shares: {listOutgoing, listIncoming, createOutgoing, revoke, accept, reject},
    settings: {get, update},
};
```

Each function accepts a typed request object (Zod schema) and returns a typed response. Zod parse runs only in development for cost-free
production builds (`z.parse` → `z.safeParse` gated on `import.meta.env.DEV`).

Tag paths on the wire are dot-separated ltree form (`Photos.Travel.Alps`) as specified in the API. The `TagPath` helper in `lib/utils.ts`
converts between display form (`/Photos/Travel/Alps`) and wire form (`Photos.Travel.Alps`).

---

## 10. Build and deployment

```
pnpm build      # vite build → dist/
```

The `dist/` folder is a static asset bundle (HTML + JS + CSS). It is deployed to any static host or CDN. The `index.html` must be served for
all routes (SPA fallback). The backend URL is injected at build time via `VITE_API_BASE_URL`.

For local development:

```
pnpm dev        # vite dev server on :5173
```

The Vite dev config proxies `/api/*` to the local backend to avoid CORS friction:

```ts
// vite.config.ts
server: {
    proxy: {
        '/api'
    :
        'http://localhost:3000',
    }
,
}
```
