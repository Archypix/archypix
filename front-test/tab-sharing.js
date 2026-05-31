const SharingTab = {
    props: ['state'],
    emits: ['update'],

    data(){
        return {
            share: {tagPath: '', recipientUsername: '', recipientInstance: '', allowBack: false, future: false},
            out: {
                create: {text: '', err: false},
                outgoing: {text: '', err: false},
                incoming: {text: '', err: false},
                action: {text: '', err: false},
            },
            acceptId: '',
            rejectId: '',
        };
    },

    methods: {
        show(key, data, isErr = false){
            this.out[key] = {
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

        async doCreateShare(){
            const r = await this.api('/api/authenticated/shares/outgoing', {
                method: 'POST',
                body: JSON.stringify({
                    tag_path: this.share.tagPath,
                    recipient_username: this.share.recipientUsername,
                    recipient_instance: this.share.recipientInstance || null,
                    allow_share_back: this.share.allowBack,
                    future: this.share.future,
                }),
            });
            this.show('create', r.data, !r.ok);
        },

        async doListOutgoing(){
            const r = await this.api('/api/authenticated/shares/outgoing');
            this.show('outgoing', r.data, !r.ok);
        },

        async doListIncoming(){
            const r = await this.api('/api/authenticated/shares/incoming');
            this.show('incoming', r.data, !r.ok);
        },

        async doAccept(){
            if(!this.acceptId) return this.show('action', 'Enter an incoming share ID.', true);
            const r = await this.api(`/api/authenticated/shares/incoming/${this.acceptId}/accept`, {method: 'POST'});
            this.show('action', r.data, !r.ok);
        },

        async doReject(){
            if(!this.rejectId) return this.show('action', 'Enter an incoming share ID.', true);
            const r = await this.api(`/api/authenticated/shares/incoming/${this.rejectId}/reject`, {method: 'POST'});
            this.show('action', r.data, !r.ok);
        },
    },

    template: `
    <div class="space-y-4">
        <div class="card">
            <h2 class="font-bold text-base mb-3 border-b pb-2">Create Outgoing Share</h2>
            <div class="grid grid-cols-2 gap-2 mb-2">
                <input class="input" placeholder="tag_path (e.g. Photos.Travel)" v-model="share.tagPath"/>
                <input class="input" placeholder="recipient_username" v-model="share.recipientUsername"/>
                <input class="input" placeholder="recipient_instance (global domain)" v-model="share.recipientInstance"/>
                <div class="flex gap-4 items-center text-xs">
                    <label><input type="checkbox" v-model="share.allowBack"/> allow_share_back</label>
                    <label><input type="checkbox" v-model="share.future"/> future</label>
                </div>
            </div>
            <button @click="doCreateShare" class="btn bg-blue-600 hover:bg-blue-700 text-white mb-2">Create Share</button>
            <pre :class="{'text-red-600': out.create.err}" class="out">{{ out.create.text }}</pre>
        </div>

        <div class="card">
            <h2 class="font-bold text-base mb-3 border-b pb-2">Outgoing Shares</h2>
            <button @click="doListOutgoing" class="btn bg-gray-200 hover:bg-gray-300 mb-2">List Outgoing</button>
            <pre :class="{'text-red-600': out.outgoing.err}" class="out">{{ out.outgoing.text }}</pre>
        </div>

        <div class="card">
            <h2 class="font-bold text-base mb-3 border-b pb-2">Incoming Shares</h2>
            <button @click="doListIncoming" class="btn bg-gray-200 hover:bg-gray-300 mb-2">List Incoming</button>
            <pre :class="{'text-red-600': out.incoming.err}" class="out mb-3">{{ out.incoming.text }}</pre>

            <div class="flex gap-2 flex-wrap items-end">
                <div class="space-y-1">
                    <label class="text-xs text-gray-600">Accept share ID</label>
                    <input class="input w-72" placeholder="incoming share UUID" v-model="acceptId"/>
                </div>
                <button @click="doAccept" class="btn bg-green-600 hover:bg-green-700 text-white">Accept</button>
                <div class="space-y-1">
                    <label class="text-xs text-gray-600">Reject share ID</label>
                    <input class="input w-72" placeholder="incoming share UUID" v-model="rejectId"/>
                </div>
                <button @click="doReject" class="btn bg-red-600 hover:bg-red-700 text-white">Reject</button>
            </div>
            <pre :class="{'text-red-600': out.action.err}" class="out mt-2">{{ out.action.text }}</pre>
        </div>
    </div>`,
};
