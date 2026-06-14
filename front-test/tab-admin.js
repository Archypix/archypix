const AdminTab = {
    props: ['state'],
    emits: ['update'],

    data(){
        return {
            activeSection: 'instance',

            // ── Instance ────────────────────────────────────────────────────────
            instanceHealth: null,
            instanceStats: null,
            consistencyStats: null,

            // ── Users ───────────────────────────────────────────────────────────
            users: [],
            newUser: {username: '', email: '', display_name: '', password: '', is_admin: false},
            editingUser: null,
            selectedUserId: '',
            userStats: null,
            userShares: null,

            // ── Jobs ────────────────────────────────────────────────────────────
            jobs: [],
            staleJobs: [],
            jobFilters: {status: '', type: '', user_id: '', limit: 50, offset: 0},

            // ── Shares ──────────────────────────────────────────────────────────
            erroredShares: [],

            // ── Federation ──────────────────────────────────────────────────────
            federationInstances: [],
            activeConnections: [],

            out: {
                instance: {text: '', err: false},
                stats: {text: '', err: false},
                consistency: {text: '', err: false},
                users: {text: '', err: false},
                userAction: {text: '', err: false},
                userStats: {text: '', err: false},
                userShares: {text: '', err: false},
                jobs: {text: '', err: false},
                staleJobs: {text: '', err: false},
                jobAction: {text: '', err: false},
                shares: {text: '', err: false},
                shareAction: {text: '', err: false},
                federation: {text: '', err: false},
                connections: {text: '', err: false},
            },
        };
    },

    methods: {
        show(key, data, isErr = false){
            this.out[key] = {
                text: isErr ? `❌ ${data}` : (typeof data === 'string' ? data : JSON.stringify(data, null, 2)),
                err: isErr,
            };
        },

        async api(path, opts = {}){
            const doFetch = () => {
                const h = {'Content-Type': 'application/json', ...(opts.headers || {})};
                if(this.state.accessToken) h['Authorization'] = `Bearer ${this.state.accessToken}`;
                return fetch(this.state.backend + path, {...opts, headers: h});
            };
            let res = await doFetch();
            if(res.status === 401){
                const ok = await this.tryRefresh();
                if(ok) res = await doFetch();
                else this.$emit('update', {accessToken: '', refreshToken: '', username: ''});
            }
            const ct = res.headers.get('content-type') || '';
            const data = ct.includes('json') ? await res.json() : await res.text();
            return {ok: res.ok, status: res.status, data};
        },

        async tryRefresh(){
            if(!this.state.refreshToken) return false;
            const res = await fetch(`${this.state.backend}/api/auth/refresh`, {
                method: 'POST',
                headers: {'Content-Type': 'application/json'},
                body: JSON.stringify({refresh_token: this.state.refreshToken}),
            });
            if(!res.ok) return false;
            const d = await res.json();
            this.$emit('update', {accessToken: d.access_token, refreshToken: d.refresh_token});
            return true;
        },

        fmtBytes(bytes){
            if(bytes < 1024) return bytes + ' B';
            if(bytes < 1048576) return (bytes / 1024).toFixed(1) + ' KB';
            if(bytes < 1073741824) return (bytes / 1048576).toFixed(1) + ' MB';
            return (bytes / 1073741824).toFixed(2) + ' GB';
        },

        statusClass(status){
            return {
                pending: 'bg-yellow-100 text-yellow-800',
                processing: 'bg-blue-100   text-blue-800',
                completed: 'bg-green-100  text-green-800',
                failed: 'bg-red-100    text-red-800',
                active: 'bg-green-100  text-green-800',
                errored: 'bg-red-100    text-red-800',
                revoked: 'bg-gray-100   text-gray-600',
            }[status] || 'bg-gray-100 text-gray-600';
        },

        // ── Instance ────────────────────────────────────────────────────────────
        async doGetInstance(){
            const r = await this.api('/api/admin/instance');
            if(r.ok) this.instanceHealth = r.data;
            this.show('instance', r.data, !r.ok);
        },

        async doGetStats(){
            const r = await this.api('/api/admin/stats');
            if(r.ok) this.instanceStats = r.data;
            this.show('stats', r.data, !r.ok);
        },

        async doGetConsistency(){
            const r = await this.api('/api/admin/consistency');
            if(r.ok) this.consistencyStats = r.data;
            this.show('consistency', r.data, !r.ok);
        },

        // ── Users ───────────────────────────────────────────────────────────────
        async doListUsers(){
            const r = await this.api('/api/admin/users');
            if(r.ok) this.users = r.data;
            else this.show('users', r.data, true);
        },

        async doCreateUser(){
            const r = await this.api('/api/admin/users', {
                method: 'POST',
                body: JSON.stringify({
                    username: this.newUser.username,
                    email: this.newUser.email,
                    display_name: this.newUser.display_name,
                    password: this.newUser.password,
                    is_admin: this.newUser.is_admin,
                }),
            });
            this.show('userAction', r.data, !r.ok);
            if(r.ok){
                this.newUser = {username: '', email: '', display_name: '', password: '', is_admin: false};
                this.doListUsers();
            }
        },

        startEditUser(u){
            this.editingUser = {id: u.id, display_name: u.display_name, is_admin: u.is_admin};
        },

        async doUpdateUser(){
            if(!this.editingUser) return;
            const r = await this.api(`/api/admin/users/${this.editingUser.id}`, {
                method: 'PATCH',
                body: JSON.stringify({
                    display_name: this.editingUser.display_name,
                    is_admin: this.editingUser.is_admin,
                }),
            });
            this.show('userAction', r.data, !r.ok);
            if(r.ok){
                this.editingUser = null;
                this.doListUsers();
            }
        },

        async doDeleteUser(userId){
            if(!confirm('Delete this user? This is irreversible.')) return;
            const r = await this.api(`/api/admin/users/${userId}`, {method: 'DELETE'});
            this.show('userAction', r.data, !r.ok);
            if(r.ok) this.doListUsers();
        },

        async doGetUserStats(){
            if(!this.selectedUserId) return this.show('userStats', 'Enter a user ID first.', true);
            const r = await this.api(`/api/admin/users/${this.selectedUserId}/stats`);
            if(r.ok) this.userStats = r.data;
            this.show('userStats', r.data, !r.ok);
        },

        async doGetUserShares(){
            if(!this.selectedUserId) return this.show('userShares', 'Enter a user ID first.', true);
            const r = await this.api(`/api/admin/users/${this.selectedUserId}/shares`);
            if(r.ok) this.userShares = r.data;
            this.show('userShares', r.data, !r.ok);
        },

        async doWakePipeline(){
            if(!this.selectedUserId) return this.show('userAction', 'Enter a user ID first.', true);
            const r = await this.api(`/api/admin/users/${this.selectedUserId}/pipeline/wake`, {method: 'POST'});
            this.show('userAction', r.data, !r.ok);
        },

        // ── Jobs ────────────────────────────────────────────────────────────────
        async doListJobs(){
            const p = new URLSearchParams();
            if(this.jobFilters.status) p.set('status', this.jobFilters.status);
            if(this.jobFilters.type) p.set('type', this.jobFilters.type);
            if(this.jobFilters.user_id) p.set('user_id', this.jobFilters.user_id);
            p.set('limit', this.jobFilters.limit);
            p.set('offset', this.jobFilters.offset);
            const r = await this.api(`/api/admin/jobs?${p}`);
            if(r.ok) this.jobs = r.data;
            else this.show('jobs', r.data, true);
        },

        async doListStaleJobs(){
            const r = await this.api('/api/admin/jobs/stale');
            if(r.ok) this.staleJobs = r.data;
            else this.show('staleJobs', r.data, true);
        },

        async doResetJob(jobId){
            const r = await this.api(`/api/admin/jobs/${jobId}/reset`, {method: 'POST'});
            this.show('jobAction', r.data, !r.ok);
            if(r.ok){
                this.doListJobs();
                this.doListStaleJobs();
            }
        },

        async doCancelJob(jobId){
            const r = await this.api(`/api/admin/jobs/${jobId}/cancel`, {method: 'POST'});
            this.show('jobAction', r.data, !r.ok);
            if(r.ok){
                this.doListJobs();
                this.doListStaleJobs();
            }
        },

        // ── Shares ──────────────────────────────────────────────────────────────
        async doListErroredShares(){
            const r = await this.api('/api/admin/shares/errored');
            if(r.ok) this.erroredShares = r.data;
            else this.show('shares', r.data, true);
        },

        async doForceReconcile(shareId){
            const r = await this.api(`/api/admin/shares/outgoing/${shareId}/force-reconcile`, {method: 'POST'});
            this.show('shareAction', r.data, !r.ok);
            if(r.ok) this.doListErroredShares();
        },

        // ── Federation ──────────────────────────────────────────────────────────
        async doListFederationInstances(){
            const r = await this.api('/api/admin/federation/instances');
            if(r.ok) this.federationInstances = r.data;
            else this.show('federation', r.data, true);
        },

        async doListActiveConnections(){
            const r = await this.api('/api/admin/federation/connections');
            if(r.ok) this.activeConnections = r.data;
            else this.show('connections', r.data, true);
        },
    },

    template: `
    <div class="space-y-4">

        <!-- Sub-nav -->
        <div class="flex gap-1 flex-wrap">
            <button v-for="s in ['instance', 'users', 'jobs', 'shares', 'federation']" :key="s"
                    @click="activeSection = s"
                    :class="activeSection === s
                        ? 'bg-indigo-600 text-white'
                        : 'bg-gray-200 hover:bg-gray-300'"
                    class="btn capitalize">{{ s }}</button>
        </div>

        <!-- ───────────────────────── INSTANCE ───────────────────────────────── -->
        <template v-if="activeSection === 'instance'">

            <div class="card">
                <h2 class="font-bold text-base mb-3 border-b pb-2">Instance Health</h2>
                <button @click="doGetInstance" class="btn bg-gray-200 hover:bg-gray-300 mb-3">Refresh</button>
                <div v-if="instanceHealth" class="grid grid-cols-2 gap-x-6 gap-y-1.5 text-sm">
                    <div class="text-gray-500">Global domain</div>
                    <div class="font-mono">{{ instanceHealth.global_domain }}</div>
                    <div class="text-gray-500">Back domain</div>
                    <div class="font-mono">{{ instanceHealth.back_domain }}</div>
                    <div class="text-gray-500">Database</div>
                    <div>
                        <span :class="instanceHealth.db_connected
                                ? 'bg-green-100 text-green-800'
                                : 'bg-red-100 text-red-800'"
                              class="text-xs font-semibold px-2 py-0.5 rounded-full">
                            {{ instanceHealth.db_connected ? 'connected' : 'DOWN' }}
                        </span>
                    </div>
                    <div class="text-gray-500">Redis</div>
                    <div>
                        <span :class="instanceHealth.redis_connected
                                ? 'bg-green-100 text-green-800'
                                : 'bg-red-100 text-red-800'"
                              class="text-xs font-semibold px-2 py-0.5 rounded-full">
                            {{ instanceHealth.redis_connected ? 'connected' : 'DOWN' }}
                        </span>
                    </div>
                    <div class="text-gray-500">Last worker activity</div>
                    <div class="font-mono text-xs">{{ instanceHealth.last_worker_activity_at || '—' }}</div>
                </div>
                <pre v-if="out.instance.err" class="out text-red-600 mt-2">{{ out.instance.text }}</pre>
            </div>

            <div class="card">
                <h2 class="font-bold text-base mb-3 border-b pb-2">
                    Instance Analytics
                    <span class="text-xs text-gray-400 font-normal ml-1">(cached 60 s)</span>
                </h2>
                <button @click="doGetStats" class="btn bg-gray-200 hover:bg-gray-300 mb-3">Refresh</button>
                <div v-if="instanceStats" class="space-y-3">
                    <div class="grid grid-cols-2 gap-x-6 gap-y-1.5 text-sm">
                        <div class="text-gray-500">Users</div>
                        <div class="font-semibold">{{ instanceStats.user_count }}</div>
                        <div class="text-gray-500">Owned pictures</div>
                        <div>{{ instanceStats.owned_picture_count }}</div>
                        <div class="text-gray-500">Received pictures</div>
                        <div>{{ instanceStats.received_picture_count }}</div>
                        <div class="text-gray-500">Total storage</div>
                        <div class="font-semibold">{{ fmtBytes(instanceStats.total_storage_bytes) }}</div>
                        <div class="text-gray-500">Dirty pictures</div>
                        <div :class="instanceStats.dirty_picture_count > 0 ? 'text-yellow-700 font-semibold' : ''">
                            {{ instanceStats.dirty_picture_count }}
                        </div>
                        <div class="text-gray-500">Errored shares</div>
                        <div :class="instanceStats.errored_share_count > 0 ? 'text-red-700 font-semibold' : ''">
                            {{ instanceStats.errored_share_count }}
                        </div>
                        <div class="text-gray-500">Pending announcements</div>
                        <div :class="instanceStats.pending_first_announcement_count > 0 ? 'text-yellow-700 font-semibold' : ''">
                            {{ instanceStats.pending_first_announcement_count }}
                        </div>
                    </div>
                    <div>
                        <div class="text-xs font-semibold text-gray-500 mb-1">Jobs by status</div>
                        <div v-if="Object.keys(instanceStats.job_counts).length === 0"
                             class="text-xs text-gray-400">No jobs.</div>
                        <div class="flex flex-wrap gap-1">
                            <span v-for="(count, status) in instanceStats.job_counts" :key="status"
                                  :class="statusClass(status)"
                                  class="text-xs font-semibold px-2 py-0.5 rounded-full">
                                {{ status }}: {{ count }}
                            </span>
                        </div>
                    </div>
                </div>
                <pre v-if="out.stats.err" class="out text-red-600 mt-2">{{ out.stats.text }}</pre>
            </div>

            <div class="card">
                <h2 class="font-bold text-base mb-3 border-b pb-2">Consistency Check</h2>
                <p class="text-xs text-gray-500 mb-2">
                    Detects orphaned state: stuck EXIF jobs, pictures without thumbnails, broken tag mappings.
                </p>
                <button @click="doGetConsistency" class="btn bg-gray-200 hover:bg-gray-300 mb-3">Run Check</button>
                <div v-if="consistencyStats" class="grid grid-cols-2 gap-x-6 gap-y-1.5 text-sm">
                    <div class="text-gray-500">Stuck EXIF pending</div>
                    <div :class="consistencyStats.stuck_exif_pending_count > 0 ? 'text-red-700 font-semibold' : 'text-green-700'">
                        {{ consistencyStats.stuck_exif_pending_count }}
                    </div>
                    <div class="text-gray-500">Missing thumbnails</div>
                    <div :class="consistencyStats.pictures_without_thumbnail_count > 0 ? 'text-red-700 font-semibold' : 'text-green-700'">
                        {{ consistencyStats.pictures_without_thumbnail_count }}
                    </div>
                    <div class="text-gray-500">Broken tag mappings</div>
                    <div :class="consistencyStats.broken_mapping_count > 0 ? 'text-red-700 font-semibold' : 'text-green-700'">
                        {{ consistencyStats.broken_mapping_count }}
                    </div>
                </div>
                <pre v-if="out.consistency.err" class="out text-red-600 mt-2">{{ out.consistency.text }}</pre>
            </div>

        </template>

        <!-- ───────────────────────── USERS ──────────────────────────────────── -->
        <template v-if="activeSection === 'users'">

            <div class="card">
                <h2 class="font-bold text-base mb-3 border-b pb-2">All Users</h2>
                <button @click="doListUsers" class="btn bg-gray-200 hover:bg-gray-300 mb-3">Refresh</button>
                <div v-if="users.length === 0" class="text-xs text-gray-400">No users loaded yet.</div>
                <div v-for="u in users" :key="u.id" class="border rounded px-3 py-2 mb-2 text-sm">
                    <div class="flex items-center justify-between gap-2">
                        <div class="min-w-0">
                            <div class="flex items-center gap-2 flex-wrap">
                                <span class="font-semibold">{{ u.username }}</span>
                                <span v-if="u.is_admin"
                                      class="text-[10px] bg-purple-100 text-purple-700 rounded px-1.5 py-0.5">admin</span>
                            </div>
                            <div class="text-xs text-gray-500 truncate">{{ u.display_name }} · {{ u.email }}</div>
                            <div class="text-xs text-gray-400">Storage: {{ fmtBytes(u.storage_bytes) }}</div>
                            <div class="font-mono text-[10px] text-gray-300 truncate">{{ u.id }}</div>
                        </div>
                        <div class="flex gap-1 shrink-0 flex-wrap justify-end">
                            <button @click="selectedUserId = u.id"
                                    class="btn text-xs py-0.5"
                                    :class="selectedUserId === u.id
                                        ? 'bg-indigo-100 text-indigo-800 ring-1 ring-indigo-400'
                                        : 'bg-gray-100 hover:bg-gray-200'">
                                {{ selectedUserId === u.id ? 'Selected' : 'Select' }}
                            </button>
                            <button @click="startEditUser(u)"
                                    class="btn bg-blue-100 hover:bg-blue-200 text-blue-800 text-xs py-0.5">Edit</button>
                            <button @click="doDeleteUser(u.id)"
                                    class="btn bg-red-600 hover:bg-red-700 text-white text-xs py-0.5">Delete</button>
                        </div>
                    </div>
                </div>
                <pre v-if="out.users.err" class="out text-red-600 mt-2">{{ out.users.text }}</pre>
            </div>

            <!-- Inline edit form -->
            <div v-if="editingUser" class="card border-2 border-blue-300">
                <h2 class="font-bold text-base mb-3 border-b pb-2">Edit User</h2>
                <div class="grid grid-cols-2 gap-2 mb-3">
                    <input class="input col-span-2" placeholder="display_name" v-model="editingUser.display_name"/>
                    <label class="flex items-center gap-2 text-sm">
                        <input type="checkbox" v-model="editingUser.is_admin"/> admin
                    </label>
                </div>
                <div class="flex gap-2">
                    <button @click="doUpdateUser" class="btn bg-blue-600 hover:bg-blue-700 text-white">Save</button>
                    <button @click="editingUser = null" class="btn bg-gray-200 hover:bg-gray-300">Cancel</button>
                </div>
            </div>

            <!-- Create user -->
            <div class="card">
                <h2 class="font-bold text-base mb-3 border-b pb-2">Create User</h2>
                <div class="grid grid-cols-2 gap-2 mb-3">
                    <input class="input" placeholder="username" v-model="newUser.username"/>
                    <input class="input" placeholder="email" v-model="newUser.email"/>
                    <input class="input" placeholder="display_name" v-model="newUser.display_name"/>
                    <input class="input" type="password" placeholder="password" v-model="newUser.password"/>
                    <label class="flex items-center gap-2 text-sm col-span-2">
                        <input type="checkbox" v-model="newUser.is_admin"/> admin
                    </label>
                </div>
                <button @click="doCreateUser" class="btn bg-green-600 hover:bg-green-700 text-white">Create</button>
            </div>

            <!-- Per-user operations -->
            <div class="card">
                <h2 class="font-bold text-base mb-3 border-b pb-2">Per-user Operations</h2>
                <p class="text-xs text-gray-400 mb-2">Click "Select" on a user above, or paste a UUID directly.</p>
                <div class="flex gap-2 mb-3">
                    <input class="input flex-1 font-mono text-xs" placeholder="User ID (UUID)" v-model="selectedUserId"/>
                    <span v-if="selectedUserId" class="self-center text-xs text-green-600 font-semibold">✓</span>
                </div>
                <div class="flex flex-wrap gap-2 mb-4">
                    <button @click="doGetUserStats"  class="btn bg-gray-200 hover:bg-gray-300">Stats</button>
                    <button @click="doGetUserShares" class="btn bg-gray-200 hover:bg-gray-300">Shares</button>
                    <button @click="doWakePipeline"  class="btn bg-indigo-600 hover:bg-indigo-700 text-white">Wake Pipeline</button>
                </div>

                <!-- User stats display -->
                <div v-if="userStats && !out.userStats.err" class="mb-4 p-3 bg-gray-50 rounded border text-sm">
                    <div class="text-xs font-semibold text-gray-500 mb-2">
                        User Stats <span class="text-gray-300 font-normal ml-1">(cached 120 s)</span>
                    </div>
                    <div class="grid grid-cols-2 gap-x-6 gap-y-1 mb-2">
                        <div class="text-gray-500">Owned pictures</div>
                        <div>{{ userStats.owned_picture_count }}</div>
                        <div class="text-gray-500">Received pictures</div>
                        <div>{{ userStats.received_picture_count }}</div>
                        <div class="text-gray-500">Storage</div>
                        <div class="font-semibold">{{ fmtBytes(userStats.storage_bytes) }}</div>
                        <div class="text-gray-500">Dirty pictures</div>
                        <div :class="userStats.dirty_picture_count > 0 ? 'text-yellow-700 font-semibold' : ''">
                            {{ userStats.dirty_picture_count }}
                        </div>
                        <div class="text-gray-500">Errored shares</div>
                        <div :class="userStats.errored_share_count > 0 ? 'text-red-700 font-semibold' : ''">
                            {{ userStats.errored_share_count }}
                        </div>
                    </div>
                    <div class="flex flex-wrap gap-1">
                        <span v-for="(c, s) in userStats.job_counts" :key="'j'+s"
                              :class="statusClass(s)"
                              class="text-[10px] font-semibold px-2 py-0.5 rounded-full">
                            jobs/{{ s }}: {{ c }}
                        </span>
                        <span v-for="(c, s) in userStats.outgoing_share_counts" :key="'os'+s"
                              class="text-[10px] bg-sky-100 text-sky-700 px-2 py-0.5 rounded-full">
                            out/{{ s }}: {{ c }}
                        </span>
                        <span v-for="(c, s) in userStats.incoming_share_counts" :key="'is'+s"
                              class="text-[10px] bg-teal-100 text-teal-700 px-2 py-0.5 rounded-full">
                            in/{{ s }}: {{ c }}
                        </span>
                    </div>
                </div>
                <pre v-if="out.userStats.err" class="out text-red-600 mb-2">{{ out.userStats.text }}</pre>

                <!-- User shares raw -->
                <div v-if="userShares && !out.userShares.err" class="mb-4">
                    <div class="text-xs font-semibold text-gray-500 mb-1">
                        Shares — outgoing: {{ userShares.outgoing.length }}, incoming: {{ userShares.incoming.length }}
                    </div>
                    <pre class="out text-xs max-h-48 overflow-y-auto">{{ JSON.stringify(userShares, null, 2) }}</pre>
                </div>
                <pre v-if="out.userShares.err" class="out text-red-600 mb-2">{{ out.userShares.text }}</pre>

                <pre v-if="out.userAction.text" :class="{'text-red-600': out.userAction.err}" class="out">{{ out.userAction.text }}</pre>
            </div>

        </template>

        <!-- ───────────────────────── JOBS ───────────────────────────────────── -->
        <template v-if="activeSection === 'jobs'">

            <div class="card">
                <h2 class="font-bold text-base mb-3 border-b pb-2">Job List</h2>
                <div class="grid grid-cols-3 gap-2 mb-2">
                    <select class="input" v-model="jobFilters.status">
                        <option value="">All statuses</option>
                        <option value="pending">pending</option>
                        <option value="processing">processing</option>
                        <option value="completed">completed</option>
                        <option value="failed">failed</option>
                    </select>
                    <select class="input" v-model="jobFilters.type">
                        <option value="">All types</option>
                        <option value="gen_thumbnail">gen_thumbnail</option>
                        <option value="ml_style">ml_style</option>
                        <option value="ml_people">ml_people</option>
                        <option value="ml_group_location">ml_group_location</option>
                        <option value="edit_picture">edit_picture</option>
                    </select>
                    <input class="input font-mono text-xs" placeholder="user_id (UUID, optional)" v-model="jobFilters.user_id"/>
                    <input class="input" type="number" placeholder="limit (1–200)" v-model.number="jobFilters.limit" min="1" max="200"/>
                    <input class="input" type="number" placeholder="offset" v-model.number="jobFilters.offset" min="0"/>
                </div>
                <div class="flex gap-2 mb-3">
                    <button @click="doListJobs" class="btn bg-gray-200 hover:bg-gray-300">Search</button>
                    <button @click="jobFilters = {status:'', type:'', user_id:'', limit:50, offset:0}"
                            class="btn bg-gray-100 hover:bg-gray-200 text-xs">Clear</button>
                </div>

                <div v-if="jobs.length === 0" class="text-xs text-gray-400">No jobs loaded.</div>
                <div v-for="j in jobs" :key="j.id" class="border rounded px-3 py-2 mb-2 text-sm">
                    <div class="flex items-start justify-between gap-2">
                        <div class="min-w-0 flex-1">
                            <div class="flex items-center gap-2 flex-wrap">
                                <span class="font-mono text-xs font-semibold">{{ j.job_type }}</span>
                                <span :class="statusClass(j.status)"
                                      class="text-[10px] font-semibold px-2 py-0.5 rounded-full">{{ j.status }}</span>
                                <span class="text-xs text-gray-400">{{ j.retry_count }}/{{ j.max_retries }} retries</span>
                            </div>
                            <div class="text-xs text-gray-500 mt-0.5">owner: {{ j.owner_username }}</div>
                            <div v-if="j.claimed_by" class="text-xs text-gray-400">claimed by: {{ j.claimed_by }}</div>
                            <div v-if="j.error_message" class="text-xs text-red-600 mt-0.5 truncate" :title="j.error_message">
                                {{ j.error_message }}
                            </div>
                            <div class="font-mono text-[10px] text-gray-300 truncate">{{ j.id }}</div>
                        </div>
                        <div class="flex gap-1 shrink-0">
                            <button v-if="j.status !== 'completed' && j.status !== 'failed'"
                                    @click="doResetJob(j.id)"
                                    class="btn bg-yellow-100 hover:bg-yellow-200 text-yellow-800 text-xs py-0.5">Reset</button>
                            <button v-if="j.status !== 'completed' && j.status !== 'failed'"
                                    @click="doCancelJob(j.id)"
                                    class="btn bg-red-600 hover:bg-red-700 text-white text-xs py-0.5">Cancel</button>
                        </div>
                    </div>
                </div>
                <pre v-if="out.jobs.err" class="out text-red-600 mt-2">{{ out.jobs.text }}</pre>
                <pre v-if="out.jobAction.text" :class="{'text-red-600': out.jobAction.err}" class="out mt-2">{{ out.jobAction.text }}</pre>
            </div>

            <div class="card">
                <h2 class="font-bold text-base mb-3 border-b pb-2">
                    Stale Jobs
                    <span class="text-xs text-gray-400 font-normal ml-1">(stuck in processing beyond timeout)</span>
                </h2>
                <button @click="doListStaleJobs" class="btn bg-gray-200 hover:bg-gray-300 mb-3">Refresh</button>
                <div v-if="staleJobs.length === 0" class="text-xs text-gray-400">No stale jobs.</div>
                <div v-for="j in staleJobs" :key="j.id"
                     class="border border-orange-200 bg-orange-50 rounded px-3 py-2 mb-2 text-sm">
                    <div class="flex items-start justify-between gap-2">
                        <div class="min-w-0">
                            <div class="flex items-center gap-2">
                                <span class="font-mono text-xs font-semibold">{{ j.job_type }}</span>
                                <span class="text-[10px] bg-orange-100 text-orange-700 px-2 py-0.5 rounded-full font-semibold">stale</span>
                            </div>
                            <div class="text-xs text-gray-500">owner: {{ j.owner_username }}</div>
                            <div class="text-xs text-gray-400">started: {{ j.started_at }}</div>
                            <div class="font-mono text-[10px] text-gray-300 truncate">{{ j.id }}</div>
                        </div>
                        <div class="flex gap-1 shrink-0">
                            <button @click="doResetJob(j.id)"
                                    class="btn bg-yellow-100 hover:bg-yellow-200 text-yellow-800 text-xs py-0.5">Reset</button>
                            <button @click="doCancelJob(j.id)"
                                    class="btn bg-red-600 hover:bg-red-700 text-white text-xs py-0.5">Cancel</button>
                        </div>
                    </div>
                </div>
                <pre v-if="out.staleJobs.err" class="out text-red-600 mt-2">{{ out.staleJobs.text }}</pre>
            </div>

        </template>

        <!-- ───────────────────────── SHARES ─────────────────────────────────── -->
        <template v-if="activeSection === 'shares'">

            <div class="card">
                <h2 class="font-bold text-base mb-3 border-b pb-2">Errored Outgoing Shares</h2>
                <p class="text-xs text-gray-500 mb-2">
                    Shares in <code>errored</code> or <code>pending_first_announcement</code> state.
                    "Force Reconcile" clears the retry backoff and immediately wakes the owner's pipeline.
                </p>
                <button @click="doListErroredShares" class="btn bg-gray-200 hover:bg-gray-300 mb-3">Refresh</button>
                <div v-if="erroredShares.length === 0" class="text-xs text-gray-400">No errored shares.</div>
                <div v-for="s in erroredShares" :key="s.id"
                     class="border border-red-200 bg-red-50 rounded px-3 py-2 mb-2 text-sm">
                    <div class="flex items-start justify-between gap-2">
                        <div class="min-w-0 flex-1">
                            <div class="font-medium truncate">{{ s.tag_path }}</div>
                            <div class="text-xs text-gray-600">
                                {{ s.owner_username }} → {{ s.recipient_username }}@{{ s.recipient_instance }}
                            </div>
                            <div class="text-xs text-gray-400">
                                next retry: {{ s.next_retry_at || 'immediately' }}
                                <span v-if="s.last_error_at"> · last error: {{ s.last_error_at }}</span>
                            </div>
                            <div class="font-mono text-[10px] text-gray-300 truncate">{{ s.id }}</div>
                        </div>
                        <button @click="doForceReconcile(s.id)"
                                class="btn bg-indigo-600 hover:bg-indigo-700 text-white text-xs py-0.5 shrink-0">
                            Force Reconcile
                        </button>
                    </div>
                </div>
                <pre v-if="out.shares.err" class="out text-red-600 mt-2">{{ out.shares.text }}</pre>
                <pre v-if="out.shareAction.text" :class="{'text-red-600': out.shareAction.err}" class="out mt-2">{{ out.shareAction.text }}</pre>
            </div>

        </template>

        <!-- ───────────────────────── FEDERATION ─────────────────────────────── -->
        <template v-if="activeSection === 'federation'">

            <!-- Active JWT connections (Redis) -->
            <div class="card">
                <h2 class="font-bold text-base mb-3 border-b pb-2">Active Connections
                    <span class="text-xs text-gray-400 font-normal ml-1">(cached federation JWTs in Redis)</span>
                </h2>
                <p class="text-xs text-gray-500 mb-2">
                    Instances this node currently holds a cached JWT for. A connection is live only while
                    the token is in Redis; it disappears once the JWT expires or is evicted.
                </p>
                <button @click="doListActiveConnections" class="btn bg-gray-200 hover:bg-gray-300 mb-3">Refresh</button>
                <div v-if="activeConnections.length === 0" class="text-xs text-gray-400">No active connections in cache.</div>
                <div class="flex flex-wrap gap-2">
                    <span v-for="domain in activeConnections" :key="domain"
                          class="text-xs bg-green-100 text-green-800 font-mono px-2 py-1 rounded-full font-semibold">
                        ● {{ domain }}
                    </span>
                </div>
                <pre v-if="out.connections.err" class="out text-red-600 mt-2">{{ out.connections.text }}</pre>
            </div>

            <!-- Share-based instances -->
            <div class="card">
                <h2 class="font-bold text-base mb-3 border-b pb-2">Share Relationships</h2>
                <p class="text-xs text-gray-500 mb-2">
                    Remote instances this node has share records with (any status). Sorted by total share volume.
                    A green dot means an active JWT is cached for that instance.
                </p>
                <button @click="doListFederationInstances" class="btn bg-gray-200 hover:bg-gray-300 mb-3">Refresh</button>
                <div v-if="federationInstances.length === 0" class="text-xs text-gray-400">No share relationships found.</div>
                <div v-for="fi in federationInstances" :key="fi.instance"
                     class="border rounded px-3 py-2 mb-2 text-sm"
                     :class="fi.errored_share_count > 0 ? 'border-red-200 bg-red-50' : ''">
                    <div class="flex items-center justify-between gap-2">
                        <div>
                            <div class="flex items-center gap-2">
                                <span v-if="activeConnections.includes(fi.instance)"
                                      class="text-green-500 font-bold text-xs" title="Active JWT in cache">●</span>
                                <span class="font-medium font-mono">{{ fi.instance }}</span>
                            </div>
                            <div class="flex gap-3 mt-1 text-xs text-gray-500">
                                <span>↑ out: {{ fi.outgoing_share_count }}</span>
                                <span>↓ in: {{ fi.incoming_share_count }}</span>
                                <span v-if="fi.errored_share_count > 0" class="text-red-600 font-semibold">
                                    ⚠ errored: {{ fi.errored_share_count }}
                                </span>
                            </div>
                        </div>
                        <span v-if="fi.errored_share_count > 0"
                              class="text-xs bg-red-100 text-red-700 px-2 py-0.5 rounded-full shrink-0">errors</span>
                    </div>
                </div>
                <pre v-if="out.federation.err" class="out text-red-600 mt-2">{{ out.federation.text }}</pre>
            </div>

        </template>

    </div>`,
};
