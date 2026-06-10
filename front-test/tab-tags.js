const TagsTab = {
    props: ['state'],
    emits: ['update'],

    data(){
        return {
            // All-tags browser
            allTags: [],
            allTagsLoaded: false,
            // Per-picture manager
            pic: {
                id: '', filename: '', tags: [], sources: null,
                showSources: false, newTag: '', busy: false, msg: '', err: false, loaded: false,
            },
            // Batch editor (power users)
            batch: {pictureIds: '', addTags: '', removeTags: '', msg: '', err: false},
        };
    },

    methods: {
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

        parseTags(input){
            return input.split(/[\n,]+/).map(t => t.trim()).filter(Boolean);
        },
        parseUuids(input){
            return input.split(/[\n,\s]+/).map(t => t.trim()).filter(Boolean);
        },

        sourceLabel(s){
            const id = s.source_id ? ` · ${String(s.source_id).slice(0, 8)}` : '';
            return `${s.source}${id}`;
        },
        sourceColor(source){
            return {
                manual: 'bg-blue-100 text-blue-700',
                rule: 'bg-green-100 text-green-700',
                segment: 'bg-purple-100 text-purple-700',
                share_mapping: 'bg-amber-100 text-amber-700',
                incoming_share: 'bg-pink-100 text-pink-700',
            }[source] || 'bg-gray-100 text-gray-600';
        },

        // ── All tags ─────────────────────────────────────────────────────────
        async loadAllTags(){
            const r = await this.api('/api/authenticated/tags');
            this.allTags = (r.ok && r.data.tags) ? r.data.tags : [];
            this.allTagsLoaded = true;
        },

        // ── Per-picture manager ──────────────────────────────────────────────
        async loadPicTags(){
            if(!this.pic.id) return;
            this.pic.msg = '';
            this.pic.err = false;
            this.pic.sources = null;
            const r = await this.api(`/api/authenticated/tags?picture_id=${encodeURIComponent(this.pic.id)}`);
            if(!r.ok){
                this.pic.err = true;
                this.pic.msg = `❌ ${typeof r.data === 'string' ? r.data : JSON.stringify(r.data)}`;
                this.pic.loaded = false;
                return;
            }
            this.pic.tags = r.data.tags || [];
            this.pic.loaded = true;
            if(this.pic.showSources) await this.loadPicSources();
        },

        async toggleSources(){
            this.pic.showSources = !this.pic.showSources;
            if(this.pic.showSources && !this.pic.sources) await this.loadPicSources();
        },

        async loadPicSources(){
            const r = await this.api(`/api/authenticated/tags?picture_id=${encodeURIComponent(this.pic.id)}&with_sources=true`);
            this.pic.sources = (r.ok && r.data.tags) ? r.data.tags : [];
        },

        async addPicTag(){
            const tag = this.pic.newTag.trim();
            if(!tag) return;
            await this.editPicTags([tag], []);
            this.pic.newTag = '';
        },
        async removePicTag(path){
            await this.editPicTags([], [path]);
        },

        async editPicTags(add_tags, remove_tags){
            this.pic.busy = true;
            this.pic.msg = '';
            this.pic.err = false;
            const r = await this.api('/api/authenticated/tags', {
                method: 'PATCH',
                body: JSON.stringify({picture_ids: [this.pic.id], add_tags, remove_tags}),
            });
            this.pic.busy = false;
            if(!r.ok){
                this.pic.err = true;
                this.pic.msg = `❌ ${typeof r.data === 'string' ? r.data : JSON.stringify(r.data)}`;
                return;
            }
            this.pic.msg = '✅ Saved (pipeline tags refresh shortly).';
            this.pic.sources = null;
            await this.loadPicTags();
        },

        // ── Batch editor ─────────────────────────────────────────────────────
        async doBatchEdit(){
            const picture_ids = this.parseUuids(this.batch.pictureIds);
            const add_tags = this.parseTags(this.batch.addTags);
            const remove_tags = this.parseTags(this.batch.removeTags);
            this.batch.msg = '';
            this.batch.err = false;
            if(!picture_ids.length){
                this.batch.err = true;
                this.batch.msg = 'Enter at least one picture ID.';
                return;
            }
            if(!add_tags.length && !remove_tags.length){
                this.batch.err = true;
                this.batch.msg = 'Enter tags to add or remove.';
                return;
            }
            const r = await this.api('/api/authenticated/tags', {
                method: 'PATCH',
                body: JSON.stringify({picture_ids, add_tags, remove_tags}),
            });
            this.batch.err = !r.ok;
            this.batch.msg = r.ok ? '✅ Applied.' : `❌ ${typeof r.data === 'string' ? r.data : JSON.stringify(r.data)}`;
        },
    },

    template: `
    <div class="space-y-4">
        <!-- Per-picture tag manager -->
        <div class="card">
            <h2 class="font-bold text-base mb-3 border-b pb-2">Picture tags</h2>
            <div class="flex gap-2 mb-3">
                <input class="input flex-1" placeholder="Picture ID (UUID)" @keyup.enter="loadPicTags" v-model="pic.id"/>
                <button @click="loadPicTags" class="btn bg-blue-600 hover:bg-blue-700 text-white">Load</button>
                <button @click="toggleSources" v-if="pic.loaded"
                        :class="pic.showSources ? 'bg-indigo-600 text-white' : 'bg-gray-100 text-gray-700 hover:bg-gray-200'"
                        class="btn">{{ pic.showSources ? 'Hide sources' : 'Show sources' }}</button>
            </div>

            <div v-if="pic.loaded">
                <!-- Folded chips -->
                <div v-if="!pic.showSources" class="flex flex-wrap gap-2 mb-3">
                    <span :key="t" class="inline-flex items-center gap-1 bg-gray-100 rounded-full pl-3 pr-1 py-1 text-xs"
                          v-for="t in pic.tags">
                        {{ t }}
                        <button @click="removePicTag(t)" title="Remove manual tag"
                                class="w-4 h-4 rounded-full bg-gray-300 hover:bg-red-400 hover:text-white text-gray-600 leading-none">×</button>
                    </span>
                    <span class="text-xs text-gray-400 italic" v-if="!pic.tags.length">No tags yet.</span>
                </div>

                <!-- Provenance -->
                <div v-else class="space-y-2 mb-3">
                    <div :key="row.path" class="flex flex-wrap items-center gap-2" v-for="row in pic.sources">
                        <span class="font-mono text-xs text-gray-800">{{ row.path }}</span>
                        <span :key="i" :class="sourceColor(s.source)" class="rounded px-1.5 py-0.5 text-[10px] font-medium"
                              v-for="(s, i) in row.sources">{{ sourceLabel(s) }}</span>
                    </div>
                    <div class="text-xs text-gray-400 italic" v-if="!pic.sources || !pic.sources.length">No tags yet.</div>
                </div>

                <div class="flex gap-2 items-center border-t pt-3">
                    <input class="input flex-1" placeholder="Tag manually, e.g. Photos.Travel.Alps"
                           @keyup.enter="addPicTag" v-model="pic.newTag"/>
                    <button @click="addPicTag" :disabled="pic.busy"
                            class="btn bg-green-600 hover:bg-green-700 text-white disabled:opacity-50">Add tag</button>
                </div>
                <p :class="pic.err ? 'text-red-600' : 'text-green-700'" class="text-xs mt-2" v-if="pic.msg">{{ pic.msg }}</p>
                <p class="text-[11px] text-gray-400 mt-1">× removes manual tags only — pipeline tags reappear unless their rule/segment is changed.</p>
            </div>
            <p :class="pic.err ? 'text-red-600' : ''" class="text-xs" v-else-if="pic.msg">{{ pic.msg }}</p>
        </div>

        <!-- All tags -->
        <div class="card">
            <div class="flex items-center justify-between border-b pb-2 mb-3">
                <h2 class="font-bold text-base">All your tags</h2>
                <button @click="loadAllTags" class="btn bg-blue-600 hover:bg-blue-700 text-white">Load</button>
            </div>
            <div class="flex flex-wrap gap-2" v-if="allTagsLoaded">
                <span :key="t" class="bg-gray-100 rounded-full px-3 py-1 text-xs font-mono" v-for="t in allTags">{{ t }}</span>
                <span class="text-xs text-gray-400 italic" v-if="!allTags.length">No tags found.</span>
            </div>
        </div>

        <!-- Batch editor -->
        <div class="card">
            <h2 class="font-bold text-base mb-3 border-b pb-2">Batch edit</h2>
            <p class="text-xs text-gray-500 mb-3">Atomically add/remove tags on many pictures. Use <code>.</code> to separate segments, e.g. <code>Photos.Travel.Alps</code>. Remove cascades to subtags; add keeps only the deepest.</p>
            <div class="grid grid-cols-1 md:grid-cols-3 gap-3 mb-3">
                <div class="space-y-1">
                    <label class="text-xs font-medium text-gray-600">Picture IDs</label>
                    <textarea class="input w-full h-24 resize-y font-mono text-xs" placeholder="uuid-1&#10;uuid-2" v-model="batch.pictureIds"></textarea>
                </div>
                <div class="space-y-1">
                    <label class="text-xs font-medium text-green-700">Add tags</label>
                    <textarea class="input w-full h-24 resize-y font-mono text-xs" placeholder="Photos.Travel.Alps&#10;Photos.2024" v-model="batch.addTags"></textarea>
                </div>
                <div class="space-y-1">
                    <label class="text-xs font-medium text-red-700">Remove tags</label>
                    <textarea class="input w-full h-24 resize-y font-mono text-xs" placeholder="OldTag.To.Remove" v-model="batch.removeTags"></textarea>
                </div>
            </div>
            <button @click="doBatchEdit" class="btn bg-blue-600 hover:bg-blue-700 text-white">Apply</button>
            <p :class="batch.err ? 'text-red-600' : 'text-green-700'" class="text-xs mt-2" v-if="batch.msg">{{ batch.msg }}</p>
        </div>
    </div>`,
};
