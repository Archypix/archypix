const AuthTab = {
    props: ['state'],
    emits: ['update'],

    data(){
        return {
            login: {username: '', password: ''},
            reg: {username: '', display: '', email: '', password: ''},
            rres: {username: '', display: '', email: '', password: ''},
            out: '',
            err: false,
        };
    },

    methods: {
        show(data, isErr = false){
            this.err = isErr;
            this.out = isErr
                ? `❌ ${data}`
                : (typeof data === 'string' ? data : JSON.stringify(data, null, 2));
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

        async doLogin(){
            const r = await this.api('/api/auth/login', {
                method: 'POST',
                body: JSON.stringify({username: this.login.username, password: this.login.password}),
            }, false);
            if(r.ok){
                this.$emit('update', {
                    accessToken: r.data.access_token,
                    refreshToken: r.data.refresh_token,
                    username: this.login.username,
                });
            }
            this.show(r.data, !r.ok);
        },

        async doRegister(){
            const r = await this.api('/api/public/users', {
                method: 'POST',
                body: JSON.stringify({
                    username: this.reg.username, display_name: this.reg.display,
                    email: this.reg.email, password: this.reg.password,
                }),
            }, false);
            this.show(r.data, !r.ok);
            if(r.ok){
                this.login.username = this.reg.username;
                this.login.password = this.reg.password;
                await this.doLogin();
            }
        },

        async doRegisterViaResolver(){
            const r = await this.api('/api/register', {
                method: 'POST',
                body: JSON.stringify({
                    username: this.rres.username, display_name: this.rres.display,
                    email: this.rres.email, password: this.rres.password,
                }),
            }, false, this.state.resolver);
            this.show(r.data, !r.ok);
            if(r.ok){
                this.login.username = this.rres.username;
                this.login.password = this.rres.password;
                await this.doLogin();
            }
        },

        async doGetMe(){
            const r = await this.api('/api/auth/me');
            if(r.ok && r.data.username) this.$emit('update', {username: r.data.username});
            this.show(r.data, !r.ok);
        },

        async doRefresh(){
            const ok = await this.tryRefresh();
            this.show(ok ? '✅ Token refreshed.' : '❌ Refresh failed — please log in again.', !ok);
        },
    },

    template: `
    <div class="space-y-4">
        <div class="card">
            <h2 class="font-bold text-base mb-3 border-b pb-2">Auth</h2>
            <div class="grid grid-cols-1 md:grid-cols-3 gap-4 mb-4">

                <div class="space-y-2">
                    <h3 class="font-semibold text-blue-700">Login</h3>
                    <input class="input w-full" placeholder="Username" v-model="login.username"/>
                    <input class="input w-full" placeholder="Password" type="password" v-model="login.password"/>
                    <button @click="doLogin" class="btn bg-blue-600 hover:bg-blue-700 text-white w-full">Login</button>
                </div>

                <div class="space-y-2">
                    <h3 class="font-semibold text-green-700">Register (direct)</h3>
                    <input class="input w-full" placeholder="Username [a-z0-9_]" v-model="reg.username"/>
                    <input class="input w-full" placeholder="Display name" v-model="reg.display"/>
                    <input class="input w-full" placeholder="Email" v-model="reg.email"/>
                    <input class="input w-full" placeholder="Password" type="password" v-model="reg.password"/>
                    <button @click="doRegister" class="btn bg-green-600 hover:bg-green-700 text-white w-full">Register &amp; Login</button>
                    <p class="text-xs text-gray-500">Hits <code>/api/public/users</code> directly.</p>
                </div>

                <div class="space-y-2">
                    <h3 class="font-semibold text-purple-700">Register via Resolver</h3>
                    <input class="input w-full" placeholder="Username [a-z0-9_]" v-model="rres.username"/>
                    <input class="input w-full" placeholder="Display name" v-model="rres.display"/>
                    <input class="input w-full" placeholder="Email" v-model="rres.email"/>
                    <input class="input w-full" placeholder="Password" type="password" v-model="rres.password"/>
                    <button @click="doRegisterViaResolver" class="btn bg-purple-600 hover:bg-purple-700 text-white w-full">Register via Resolver</button>
                    <p class="text-xs text-gray-500">Resolver picks the least-loaded backend.</p>
                </div>
            </div>

            <div class="flex gap-2 mb-2">
                <button @click="doGetMe"    class="btn bg-gray-200 hover:bg-gray-300">GET /api/auth/me</button>
                <button @click="doRefresh"  class="btn bg-gray-200 hover:bg-gray-300">Refresh token</button>
            </div>
            <pre :class="{'text-red-600': err}" class="out">{{ out }}</pre>
        </div>
    </div>`,
};
