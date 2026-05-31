const FederationTab = {
    props: ['state'],
    emits: ['update'],

    data(){
        return {
            wfAddress: '',
            profileUsername: '',
            out: {
                wf: {text: '', err: false},
                profile: {text: '', err: false},
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

        async api(path, opts = {}, auth = true, base = null){
            const doFetch = () => {
                const h = {'Content-Type': 'application/json', ...(opts.headers || {})};
                if(auth && this.state.accessToken) h['Authorization'] = `Bearer ${this.state.accessToken}`;
                return fetch((base || this.state.backend) + path, {...opts, headers: h});
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

        async doWebFinger(){
            const m = this.wfAddress.match(/^@?([^:]+):(.+)$/);
            if(!m) return this.show('wf', 'Format: @username:domain', true);
            const url = `${this.state.resolver}/.well-known/webfinger?resource=${encodeURIComponent(`archypix:@${m[1]}:${m[2]}`)}`;
            try{
                this.show('wf', `Querying: ${url}`);
                const res = await fetch(url);
                const ct = res.headers.get('content-type') || '';
                const data = ct.includes('json') ? await res.json() : await res.text();
                this.show('wf', data, !res.ok);
            }catch(e){
                this.show('wf', e.message, true);
            }
        },

        async doGetProfile(){
            if(!this.profileUsername) return this.show('profile', 'Enter a username.', true);
            const r = await this.api(`/api/public/users/${this.profileUsername}`, {}, false);
            this.show('profile', r.data, !r.ok);
        },
    },

    template: `
    <div class="space-y-4">
        <div class="card">
            <h2 class="font-bold text-base mb-3 border-b pb-2">WebFinger Lookup</h2>
            <p class="text-xs text-gray-500 mb-2">Queries the Resolver. Format: <code>@username:domain</code></p>
            <div class="flex gap-2 mb-2">
                <input class="input flex-1" placeholder="@username:domain" v-model="wfAddress"/>
                <button @click="doWebFinger" class="btn bg-blue-600 hover:bg-blue-700 text-white">Lookup</button>
            </div>
            <pre :class="{'text-red-600': out.wf.err}" class="out">{{ out.wf.text }}</pre>
        </div>

        <div class="card">
            <h2 class="font-bold text-base mb-3 border-b pb-2">Public Profile</h2>
            <div class="flex gap-2 mb-2">
                <input class="input flex-1" placeholder="username" v-model="profileUsername"/>
                <button @click="doGetProfile" class="btn bg-gray-200 hover:bg-gray-300">Lookup</button>
            </div>
            <pre :class="{'text-red-600': out.profile.err}" class="out">{{ out.profile.text }}</pre>
        </div>
    </div>`,
};
