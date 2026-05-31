const SettingsTab = {
    props: ['state'],
    emits: ['update'],

    data(){
        return {
            versioningMode: 'none',
            out: {text: '', err: false},
        };
    },

    methods: {
        show(data, isErr = false){
            this.out = {
                text: isErr ? `❌ ${data}` : (typeof data === 'string' ? data : JSON.stringify(data, null, 2)),
                err: isErr,
            };
        },

        async api(path, opts = {}, auth = true){
            const doFetch = () => {
                const h = {'Content-Type': 'application/json', ...(opts.headers || {})};
                if(auth && this.state.accessToken) h['Authorization'] = `Bearer ${this.state.accessToken}`;
                return fetch(this.state.backend + path, {...opts, headers: h});
            };
            let res = await doFetch();
            if(res.status === 401 && auth){
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

        async doLoad(){
            const r = await this.api('/api/authenticated/settings');
            if(r.ok && r.data.versioning_mode) this.versioningMode = r.data.versioning_mode;
            this.show(r.data, !r.ok);
        },

        async doSave(){
            const r = await this.api('/api/authenticated/settings', {
                method: 'PATCH',
                body: JSON.stringify({versioning_mode: this.versioningMode}),
            });
            this.show(r.data, !r.ok);
        },
    },

    template: `
    <div class="space-y-4">
        <div class="card">
            <h2 class="font-bold text-base mb-3 border-b pb-2">User Settings</h2>
            <div class="flex gap-3 mb-3 flex-wrap items-center">
                <button @click="doLoad" class="btn bg-gray-200 hover:bg-gray-300">Load</button>
                <label class="text-sm">versioning_mode:
                    <select class="input ml-1" v-model="versioningMode">
                        <option value="none">none</option>
                        <option value="original_copy">original_copy</option>
                        <option value="full_versioning">full_versioning</option>
                    </select>
                </label>
                <button @click="doSave" class="btn bg-green-600 hover:bg-green-700 text-white">Save</button>
            </div>
            <pre :class="{'text-red-600': out.err}" class="out">{{ out.text }}</pre>
        </div>
    </div>`,
};
