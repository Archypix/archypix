const AdminTab = {
    props: ['state'],
    emits: ['update'],

    data(){
        return {
            users: [],
            createForm: {username: '', email: '', displayName: '', password: '', isAdmin: false},
            updateForm: {userId: '', displayName: '', isAdmin: false},
            deleteUserId: '',
            out: {
                list: {text: '', err: false}, create: {text: '', err: false},
                update: {text: '', err: false}, delete: {text: '', err: false}
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
            const h = {'Content-Type': 'application/json'};
            if(this.state.accessToken) h['Authorization'] = `Bearer ${this.state.accessToken}`;
            const res = await fetch(this.state.backend + path, {...opts, headers: h});
            const ct = res.headers.get('content-type') || '';
            const data = ct.includes('json') ? await res.json() : await res.text();
            return {ok: res.ok, status: res.status, data};
        },

        async doListUsers(){
            const r = await this.api('/api/admin/users');
            this.show('list', r.data, !r.ok);
            if(r.ok && Array.isArray(r.data)) this.users = r.data;
            else this.users = [];
        },

        async doCreateUser(){
            const r = await this.api('/api/admin/users', {
                method: 'POST',
                body: JSON.stringify({
                    username: this.createForm.username,
                    email: this.createForm.email,
                    display_name: this.createForm.displayName,
                    password: this.createForm.password,
                    is_admin: this.createForm.isAdmin,
                }),
            });
            this.show('create', r.data, !r.ok);
            if(r.ok) await this.doListUsers();
        },

        async doUpdateUser(){
            if(!this.updateForm.userId) return this.show('update', 'Enter a user ID.', true);
            const body = {};
            if(this.updateForm.displayName) body.display_name = this.updateForm.displayName;
            body.is_admin = this.updateForm.isAdmin;
            const r = await this.api(`/api/admin/users/${this.updateForm.userId}`, {
                method: 'PATCH',
                body: JSON.stringify(body),
            });
            this.show('update', r.data, !r.ok);
            if(r.ok) await this.doListUsers();
        },

        async doDeleteUser(){
            if(!this.deleteUserId) return this.show('delete', 'Enter a user ID.', true);
            const r = await this.api(`/api/admin/users/${this.deleteUserId}`, {method: 'DELETE'});
            this.show('delete', r.data, !r.ok);
            if(r.ok){
                this.deleteUserId = '';
                await this.doListUsers();
            }
        },

        selectUser(user){
            this.updateForm.userId = user.id;
            this.updateForm.displayName = user.display_name || '';
            this.updateForm.isAdmin = user.is_admin || false;
        },
    },

    template: `
    <div class="space-y-4">
        <div class="card">
            <h2 class="font-bold text-base mb-3 border-b pb-2">Users</h2>
            <p class="text-xs text-gray-500 mb-2">Uses the logged-in user's token. Requires <code>is_admin = true</code>.</p>
            <button @click="doListUsers" class="btn bg-gray-200 hover:bg-gray-300 mb-2">List Users</button>
            <pre :class="{'text-red-600': out.list.err}" class="out mb-3">{{ out.list.text }}</pre>

            <div class="overflow-x-auto" v-if="users.length">
                <table class="text-xs w-full border-collapse">
                    <thead>
                        <tr class="bg-gray-100 text-left">
                            <th class="p-2 border">Username</th>
                            <th class="p-2 border">Display name</th>
                            <th class="p-2 border">Email</th>
                            <th class="p-2 border">Admin</th>
                            <th class="p-2 border">Actions</th>
                        </tr>
                    </thead>
                    <tbody>
                        <tr :key="u.id" v-for="u in users" class="hover:bg-gray-50">
                            <td class="p-2 border font-mono">{{ u.username }}</td>
                            <td class="p-2 border">{{ u.display_name }}</td>
                            <td class="p-2 border">{{ u.email }}</td>
                            <td class="p-2 border text-center">{{ u.is_admin ? '✓' : '' }}</td>
                            <td class="p-2 border">
                                <button @click="selectUser(u)" class="btn bg-blue-100 hover:bg-blue-200 text-blue-800 text-xs">Edit</button>
                                <button @click="deleteUserId = u.id; doDeleteUser()" class="btn bg-red-100 hover:bg-red-200 text-red-800 text-xs ml-1">Delete</button>
                            </td>
                        </tr>
                    </tbody>
                </table>
            </div>
        </div>

        <div class="card">
            <h2 class="font-bold text-base mb-3 border-b pb-2">Create User</h2>
            <div class="grid grid-cols-2 gap-2 mb-2">
                <input class="input" placeholder="username" v-model="createForm.username"/>
                <input class="input" placeholder="email" v-model="createForm.email"/>
                <input class="input" placeholder="display name" v-model="createForm.displayName"/>
                <input class="input" placeholder="password" type="password" v-model="createForm.password"/>
                <label class="flex items-center gap-2 text-xs col-span-2">
                    <input type="checkbox" v-model="createForm.isAdmin"/> Is admin
                </label>
            </div>
            <button @click="doCreateUser" class="btn bg-green-600 hover:bg-green-700 text-white mb-2">Create</button>
            <pre :class="{'text-red-600': out.create.err}" class="out">{{ out.create.text }}</pre>
        </div>

        <div class="card">
            <h2 class="font-bold text-base mb-3 border-b pb-2">Update User</h2>
            <p class="text-xs text-gray-500 mb-2">Click "Edit" on a row above to populate this form.</p>
            <div class="grid grid-cols-2 gap-2 mb-2">
                <input class="input font-mono text-xs col-span-2" placeholder="user UUID" v-model="updateForm.userId"/>
                <input class="input" placeholder="new display name (optional)" v-model="updateForm.displayName"/>
                <label class="flex items-center gap-2 text-xs">
                    <input type="checkbox" v-model="updateForm.isAdmin"/> Is admin
                </label>
            </div>
            <button @click="doUpdateUser" class="btn bg-blue-600 hover:bg-blue-700 text-white mb-2">Update</button>
            <pre :class="{'text-red-600': out.update.err}" class="out">{{ out.update.text }}</pre>
        </div>

        <div class="card">
            <h2 class="font-bold text-base mb-3 border-b pb-2">Delete User</h2>
            <div class="flex gap-2 mb-2">
                <input class="input flex-1 font-mono text-xs" placeholder="user UUID" v-model="deleteUserId"/>
                <button @click="doDeleteUser" class="btn bg-red-600 hover:bg-red-700 text-white">Delete</button>
            </div>
            <pre :class="{'text-red-600': out.delete.err}" class="out">{{ out.delete.text }}</pre>
        </div>
    </div>`,
};
