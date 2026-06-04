const SharingTab = {
    props: ['state'],
    emits: ['update'],

    data(){
        return {
            share: {tagPath: '', recipientUsername: '', recipientInstance: '', allowBack: false, future: false},
            outgoingShares: [],
            incomingShares: [],
            out: {
                create: {text: '', err: false},
                action: {text: '', err: false},
            },
        };
    },

    computed: {
        statusBadgeClass(){
            return (status) => ({
                pending: 'bg-yellow-100 text-yellow-800',
                active: 'bg-green-100  text-green-800',
                revoked: 'bg-red-100    text-red-800',
                tombstoned: 'bg-gray-100   text-gray-600',
            }[status] || 'bg-gray-100 text-gray-600');
        },
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
            if(r.ok) this.doListOutgoing();
        },

        async doListOutgoing(){
            const r = await this.api('/api/authenticated/shares/outgoing');
            if(r.ok) this.outgoingShares = r.data;
            else this.show('action', r.data, true);
        },

        async doListIncoming(){
            const r = await this.api('/api/authenticated/shares/incoming');
            if(r.ok) this.incomingShares = r.data;
            else this.show('action', r.data, true);
        },

        async doRevoke(shareId){
            const r = await this.api(`/api/authenticated/shares/outgoing/${shareId}/revoke`, {method: 'POST'});
            this.show('action', r.data, !r.ok);
            if(r.ok) this.doListOutgoing();
        },

        async doAccept(shareId){
            const r = await this.api(`/api/authenticated/shares/incoming/${shareId}/accept`, {method: 'POST'});
            this.show('action', r.data, !r.ok);
            if(r.ok) this.doListIncoming();
        },

        async doReject(shareId){
            const r = await this.api(`/api/authenticated/shares/incoming/${shareId}/reject`, {method: 'POST'});
            this.show('action', r.data, !r.ok);
            if(r.ok) this.doListIncoming();
        },
    },

    template: `
    <div class="space-y-4">

        <!-- Create -->
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

        <!-- Outgoing -->
        <div class="card">
            <h2 class="font-bold text-base mb-3 border-b pb-2">Outgoing Shares</h2>
            <button @click="doListOutgoing" class="btn bg-gray-200 hover:bg-gray-300 mb-3">Refresh</button>
            <div v-if="outgoingShares.length === 0" class="text-xs text-gray-400">No outgoing shares.</div>
            <div v-for="s in outgoingShares" :key="s.id"
                 class="flex items-center justify-between border rounded px-3 py-2 mb-2 text-sm gap-2">
                <div class="min-w-0">
                    <div class="font-mono text-xs text-gray-400 truncate">{{ s.id }}</div>
                    <div class="font-medium truncate">{{ s.tag_path }}</div>
                    <div class="text-xs text-gray-500">→ {{ s.recipient_username }}@{{ s.recipient_instance }}</div>
                </div>
                <div class="flex items-center gap-2 shrink-0">
                    <span :class="statusBadgeClass(s.status)"
                          class="text-xs font-semibold px-2 py-0.5 rounded-full capitalize">{{ s.status }}</span>
                    <button v-if="s.status === 'active'"
                            @click="doRevoke(s.id)"
                            class="btn bg-red-600 hover:bg-red-700 text-white text-xs py-0.5">Revoke</button>
                </div>
            </div>
        </div>

        <!-- Incoming -->
        <div class="card">
            <h2 class="font-bold text-base mb-3 border-b pb-2">Incoming Shares</h2>
            <button @click="doListIncoming" class="btn bg-gray-200 hover:bg-gray-300 mb-3">Refresh</button>
            <div v-if="incomingShares.length === 0" class="text-xs text-gray-400">No incoming shares.</div>
            <div v-for="s in incomingShares" :key="s.id"
                 class="flex items-center justify-between border rounded px-3 py-2 mb-2 text-sm gap-2">
                <div class="min-w-0">
                    <div class="font-mono text-xs text-gray-400 truncate">{{ s.id }}</div>
                    <div class="font-medium truncate">from {{ s.sender_username }}@{{ s.sender_instance }}</div>
                </div>
                <div class="flex items-center gap-2 shrink-0">
                    <span :class="statusBadgeClass(s.status)"
                          class="text-xs font-semibold px-2 py-0.5 rounded-full capitalize">{{ s.status }}</span>
                    <template v-if="s.status === 'pending'">
                        <button @click="doAccept(s.id)"
                                class="btn bg-green-600 hover:bg-green-700 text-white text-xs py-0.5">Accept</button>
                        <button @click="doReject(s.id)"
                                class="btn bg-red-500 hover:bg-red-600 text-white text-xs py-0.5">Reject</button>
                    </template>
                    <button v-else-if="s.status === 'active'"
                            @click="doReject(s.id)"
                            class="btn bg-gray-400 hover:bg-gray-500 text-white text-xs py-0.5">Remove</button>
                </div>
            </div>
        </div>

        <!-- Action feedback -->
        <div class="card" v-if="out.action.text">
            <pre :class="{'text-red-600': out.action.err}" class="out">{{ out.action.text }}</pre>
        </div>
    </div>`,
};
