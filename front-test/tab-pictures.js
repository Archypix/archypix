const PicturesTab = {
    props: ['state'],
    emits: ['update'],

    data(){
        return {
            uploadProgress: '',
            listPage: 1, listSize: 20, listThumbnail: 'small', listScope: 'all',
            picGrid: [],
            picId: '',
            picUrl: {id: '', variant: 'original', imgSrc: null},
            // Inline tag pane for the selected picture.
            tagPane: {
                pictureId: '', filename: '', tags: [], sources: null,
                showSources: false, newTag: '', busy: false, msg: '', err: false,
            },
            out: {
                upload: {text: '', err: false}, list: {text: '', err: false},
                detail: {text: '', err: false}, picUrl: {text: '', err: false}
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

        async doUpload(){
            const file = this.$refs.fileInput.files[0];
            if(!file) return this.show('upload', 'No file selected.', true);
            try{
                this.uploadProgress = '1/3 Requesting upload URL…';
                const r1 = await this.api('/api/authenticated/pictures/uploads', {
                    method: 'POST', body: JSON.stringify({filename: file.name}),
                });
                if(!r1.ok) return this.show('upload', `HTTP ${r1.status}: ${JSON.stringify(r1.data)}`, true);

                const {picture_id, presigned_url} = r1.data;
                this.uploadProgress = `2/3 Uploading to S3… id=${picture_id}`;
                const put = await fetch(presigned_url, {
                    method: 'PUT', body: file,
                    headers: {'Content-Type': file.type || 'application/octet-stream'},
                });
                if(!put.ok) return this.show('upload', `S3 PUT failed: HTTP ${put.status}`, true);

                this.uploadProgress = '3/3 Completing…';
                const body = {mime_type: file.type || null, file_size: file.size || null};
                if(file.type.startsWith('image/')){
                    try{
                        Object.assign(body, await this.getImageDimensions(file));
                    }catch(_){
                    }
                }
                const r3 = await this.api(`/api/authenticated/pictures/uploads/${picture_id}/complete`, {
                    method: 'POST', body: JSON.stringify(body),
                });
                this.uploadProgress = r3.ok ? '✅ Done!' : '❌ Complete step failed.';
                this.show('upload', r3.data, !r3.ok);
            }catch(e){
                this.uploadProgress = '';
                this.show('upload', e.message, true);
            }
        },

        getImageDimensions(file){
            return new Promise((res, rej) => {
                const url = URL.createObjectURL(file);
                const img = new Image();
                img.onload = () => {
                    res({width: img.naturalWidth, height: img.naturalHeight});
                    URL.revokeObjectURL(url);
                };
                img.onerror = rej;
                img.src = url;
            });
        },

        async doListPictures(){
            let qs = `?page=${this.listPage}&page_size=${this.listSize}${this.listThumbnail ? '&thumbnail=' + this.listThumbnail : ''}`;
            if(this.listScope === 'owned') qs += '&owned_only=true';
            else if(this.listScope === 'shared') qs += '&shared_with_me=true';
            const r = await this.api('/api/authenticated/pictures' + qs);
            this.show('list', r.data, !r.ok);
            this.picGrid = (r.ok && r.data.items) ? r.data.items : [];
        },

        selectPicture(id){
            this.picId = id;
            this.picUrl.id = id;
            const pic = this.picGrid.find(p => p.id === id);
            this.openTagPane(id, pic ? pic.filename : '');
            this.doGetPicture();
        },

        // ── Inline tag pane ──────────────────────────────────────────────────
        openTagPane(id, filename){
            this.tagPane.pictureId = id;
            this.tagPane.filename = filename || '';
            this.tagPane.showSources = false;
            this.tagPane.sources = null;
            this.tagPane.newTag = '';
            this.tagPane.msg = '';
            this.tagPane.err = false;
            this.loadPaneTags();
        },

        async loadPaneTags(){
            const id = this.tagPane.pictureId;
            if(!id) return;
            const r = await this.api(`/api/authenticated/tags?picture_id=${encodeURIComponent(id)}`);
            this.tagPane.tags = (r.ok && r.data.tags) ? r.data.tags : [];
            if(this.tagPane.showSources) await this.loadPaneSources();
        },

        async toggleSources(){
            this.tagPane.showSources = !this.tagPane.showSources;
            if(this.tagPane.showSources && !this.tagPane.sources) await this.loadPaneSources();
        },

        async loadPaneSources(){
            const id = this.tagPane.pictureId;
            const r = await this.api(`/api/authenticated/tags?picture_id=${encodeURIComponent(id)}&with_sources=true`);
            this.tagPane.sources = (r.ok && r.data.tags) ? r.data.tags : [];
        },

        async addManualTag(){
            const tag = this.tagPane.newTag.trim();
            if(!tag) return;
            await this.editPaneTags([tag], []);
            this.tagPane.newTag = '';
        },

        async removePaneTag(path){
            await this.editPaneTags([], [path]);
        },

        async editPaneTags(add_tags, remove_tags){
            this.tagPane.busy = true;
            this.tagPane.msg = '';
            this.tagPane.err = false;
            const r = await this.api('/api/authenticated/tags', {
                method: 'PATCH',
                body: JSON.stringify({picture_ids: [this.tagPane.pictureId], add_tags, remove_tags}),
            });
            this.tagPane.busy = false;
            if(!r.ok){
                this.tagPane.err = true;
                this.tagPane.msg = `❌ ${typeof r.data === 'string' ? r.data : JSON.stringify(r.data)}`;
                return;
            }
            // Manual edits apply immediately; pipeline-derived tags may lag a moment.
            this.tagPane.msg = '✅ Saved (pipeline tags refresh shortly).';
            this.tagPane.sources = null;
            await this.loadPaneTags();
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

        async doGetPicture(){
            if(!this.picId) return this.show('detail', 'Enter a picture ID.', true);
            const r = await this.api(`/api/authenticated/pictures/${this.picId}`);
            this.show('detail', r.data, !r.ok);
        },

        async doGetPictureUrl(){
            if(!this.picUrl.id) return this.show('picUrl', 'Enter a picture ID.', true);
            this.picUrl.imgSrc = null;
            const r = await this.api(`/api/authenticated/pictures/${this.picUrl.id}/url?variant=${this.picUrl.variant}`);
            this.show('picUrl', r.data, !r.ok);
            if(r.ok && r.data.url) this.picUrl.imgSrc = r.data.url;
        },
    },

    template: `
    <div class="space-y-4">
        <div class="card">
            <h2 class="font-bold text-base mb-3 border-b pb-2">Upload</h2>
            <div class="flex gap-2 items-center mb-1">
                <input class="text-xs" ref="fileInput" type="file"/>
                <button @click="doUpload" class="btn bg-blue-600 hover:bg-blue-700 text-white">Upload</button>
            </div>
            <div class="text-xs text-gray-500 mb-1">{{ uploadProgress }}</div>
            <pre :class="{'text-red-600': out.upload.err}" class="out">{{ out.upload.text }}</pre>
        </div>

        <div class="card">
            <h2 class="font-bold text-base mb-3 border-b pb-2">List</h2>
            <div class="flex gap-2 mb-2 flex-wrap">
                <input class="input w-16" min="1" placeholder="page" type="number" v-model.number="listPage"/>
                <input class="input w-20" min="1" placeholder="page_size" type="number" v-model.number="listSize"/>
                <select class="input" v-model="listThumbnail">
                    <option value="">no thumbnail</option>
                    <option value="small">small</option>
                    <option value="medium">medium</option>
                    <option value="large">large</option>
                </select>
                <select class="input" v-model="listScope" title="ownership filter">
                    <option value="all">all pictures</option>
                    <option value="owned">owned only</option>
                    <option value="shared">shared with me</option>
                </select>
                <button @click="doListPictures" class="btn bg-blue-600 hover:bg-blue-700 text-white">List</button>
            </div>
            <pre :class="{'text-red-600': out.list.err}" class="out mb-2">{{ out.list.text }}</pre>
            <div class="grid grid-cols-3 md:grid-cols-6 gap-2">
                <div :key="pic.id" @click="selectPicture(pic.id)"
                     :class="tagPane.pictureId === pic.id ? 'ring-2 ring-blue-500' : ''"
                     class="border rounded overflow-hidden cursor-pointer hover:shadow-md"
                     v-for="pic in picGrid">
                    <div class="relative">
                        <img :src="pic.thumbnail_url" class="w-full h-20 object-cover bg-gray-200" v-if="pic.thumbnail_url"/>
                        <div class="w-full h-20 bg-gray-200 flex items-center justify-center text-xs text-gray-400" v-else>no thumb</div>
                        <span v-if="pic.owned === false"
                              class="absolute top-1 left-1 text-[9px] bg-pink-600 text-white rounded px-1 py-0.5 leading-none"
                              :title="'shared by @' + pic.owner_username + ':' + pic.owner_instance">shared</span>
                    </div>
                    <div class="p-1 text-xs truncate text-gray-700">{{ pic.filename }}</div>
                    <div v-if="pic.owned === false" class="px-1 pb-1 text-[10px] truncate text-pink-600">by {{ pic.owner_username }}</div>
                </div>
            </div>
        </div>

        <!-- Inline tag pane: opens when a picture is selected -->
        <div class="card" v-if="tagPane.pictureId">
            <div class="flex items-center justify-between border-b pb-2 mb-3">
                <h2 class="font-bold text-base">
                    🏷 Tags
                    <span class="text-gray-400 font-normal text-xs">— {{ tagPane.filename || tagPane.pictureId.slice(0, 8) }}</span>
                </h2>
                <div class="flex gap-2">
                    <button @click="toggleSources"
                            :class="tagPane.showSources ? 'bg-indigo-600 text-white' : 'bg-gray-100 text-gray-700 hover:bg-gray-200'"
                            class="btn">{{ tagPane.showSources ? 'Hide sources' : 'Show sources' }}</button>
                    <button @click="loadPaneTags" class="btn bg-gray-100 text-gray-700 hover:bg-gray-200">↻</button>
                </div>
            </div>

            <!-- Folded view: tag chips with manual remove -->
            <div v-if="!tagPane.showSources">
                <div class="flex flex-wrap gap-2 mb-3">
                    <span :key="t" class="inline-flex items-center gap-1 bg-gray-100 rounded-full pl-3 pr-1 py-1 text-xs"
                          v-for="t in tagPane.tags">
                        {{ t }}
                        <button @click="removePaneTag(t)" title="Remove manual tag"
                                class="w-4 h-4 rounded-full bg-gray-300 hover:bg-red-400 hover:text-white text-gray-600 leading-none">×</button>
                    </span>
                    <span class="text-xs text-gray-400 italic" v-if="!tagPane.tags.length">No tags yet.</span>
                </div>
            </div>

            <!-- Provenance view: each path with the sources asserting it -->
            <div v-else class="mb-3 space-y-2">
                <div :key="row.path" class="flex flex-wrap items-center gap-2" v-for="row in tagPane.sources">
                    <span class="font-mono text-xs text-gray-800">{{ row.path }}</span>
                    <span :key="i" :class="sourceColor(s.source)" class="rounded px-1.5 py-0.5 text-[10px] font-medium"
                          v-for="(s, i) in row.sources">{{ sourceLabel(s) }}</span>
                </div>
                <div class="text-xs text-gray-400 italic" v-if="!tagPane.sources || !tagPane.sources.length">No tags yet.</div>
            </div>

            <!-- Manual tagging -->
            <div class="flex gap-2 items-center border-t pt-3">
                <input class="input flex-1" placeholder="Tag manually, e.g. Photos.Travel.Alps"
                       @keyup.enter="addManualTag" v-model="tagPane.newTag"/>
                <button @click="addManualTag" :disabled="tagPane.busy"
                        class="btn bg-green-600 hover:bg-green-700 text-white disabled:opacity-50">Add tag</button>
            </div>
            <p :class="tagPane.err ? 'text-red-600' : 'text-green-700'" class="text-xs mt-2" v-if="tagPane.msg">{{ tagPane.msg }}</p>
            <p class="text-[11px] text-gray-400 mt-1">× removes manual tags only — pipeline tags reappear unless their rule/segment is changed.</p>
        </div>

        <div class="card">
            <h2 class="font-bold text-base mb-3 border-b pb-2">Details</h2>
            <div class="flex gap-2 mb-2">
                <input class="input flex-1" placeholder="Picture ID (UUID)" v-model="picId"/>
                <button @click="doGetPicture" class="btn bg-blue-600 hover:bg-blue-700 text-white">Details</button>
            </div>
            <pre :class="{'text-red-600': out.detail.err}" class="out">{{ out.detail.text }}</pre>
        </div>

        <div class="card">
            <h2 class="font-bold text-base mb-3 border-b pb-2">Picture URL</h2>
            <div class="flex gap-2 flex-wrap mb-3">
                <input class="input flex-1" placeholder="Picture ID (UUID)" v-model="picUrl.id"/>
                <select class="input" v-model="picUrl.variant">
                    <option value="original">original</option>
                    <option value="small">small</option>
                    <option value="medium">medium</option>
                    <option value="large">large</option>
                </select>
                <button @click="doGetPictureUrl" class="btn bg-blue-600 hover:bg-blue-700 text-white">Get URL</button>
            </div>
            <pre :class="{'text-red-600': out.picUrl.err}" class="out mb-3">{{ out.picUrl.text }}</pre>
            <img :src="picUrl.imgSrc" @error="picUrl.imgSrc = null" alt="preview"
                 class="max-h-96 rounded border shadow mx-auto block" v-if="picUrl.imgSrc"/>
        </div>
    </div>`,
};
