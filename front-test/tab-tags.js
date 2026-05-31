const TagsTab = {
    props: ['state'],
    emits: ['update'],

    data(){
        return {
            filterPictureId: '',
            editPictureIds: '',
            addTagsInput: '',
            removeTagsInput: '',
            out: {list: {text: '', err: false}, edit: {text: '', err: false}},
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

        parseTags(input){
            return input.split(/[\n,]+/).map(t => t.trim()).filter(Boolean);
        },

        parseUuids(input){
            return input.split(/[\n,\s]+/).map(t => t.trim()).filter(Boolean);
        },

        async doListTags(){
            const qs = this.filterPictureId ? `?picture_id=${encodeURIComponent(this.filterPictureId)}` : '';
            const r = await this.api('/api/authenticated/tags' + qs);
            this.show('list', r.data, !r.ok);
        },

        async doEditTags(){
            const picture_ids = this.parseUuids(this.editPictureIds);
            const add_tags = this.parseTags(this.addTagsInput);
            const remove_tags = this.parseTags(this.removeTagsInput);

            if(!picture_ids.length) return this.show('edit', 'Enter at least one picture ID.', true);
            if(!add_tags.length && !remove_tags.length)
                return this.show('edit', 'Enter tags to add or remove.', true);

            const r = await this.api('/api/authenticated/tags', {
                method: 'PATCH',
                body: JSON.stringify({picture_ids, add_tags, remove_tags}),
            });
            this.show('edit', r.data, !r.ok);
        },
    },

    template: `
    <div class="space-y-4">
        <div class="card">
            <h2 class="font-bold text-base mb-3 border-b pb-2">List Tags</h2>
            <p class="text-xs text-gray-500 mb-2">Optionally filter by picture ID.</p>
            <div class="flex gap-2 mb-2">
                <input class="input flex-1" placeholder="Picture ID (UUID, optional)" v-model="filterPictureId"/>
                <button @click="doListTags" class="btn bg-blue-600 hover:bg-blue-700 text-white">List</button>
            </div>
            <pre :class="{'text-red-600': out.list.err}" class="out">{{ out.list.text }}</pre>
        </div>

        <div class="card">
            <h2 class="font-bold text-base mb-3 border-b pb-2">Edit Tags (batch)</h2>
            <p class="text-xs text-gray-500 mb-3">Atomically add/remove tags on a batch of pictures. Tags: dot-separated ltree, e.g. <code>Photos.Travel.Alps</code>. Remove cascades to subtags; add keeps only the deepest.</p>
            <div class="grid grid-cols-1 md:grid-cols-3 gap-3 mb-3">
                <div class="space-y-1">
                    <label class="text-xs font-medium text-gray-600">Picture IDs (one per line or comma-separated)</label>
                    <textarea class="input w-full h-28 resize-y font-mono text-xs"
                              placeholder="uuid-1&#10;uuid-2"
                              v-model="editPictureIds"></textarea>
                </div>
                <div class="space-y-1">
                    <label class="text-xs font-medium text-green-700">Add tags (one per line or comma-separated)</label>
                    <textarea class="input w-full h-28 resize-y font-mono text-xs"
                              placeholder="Photos.Travel.Alps&#10;Photos.2024"
                              v-model="addTagsInput"></textarea>
                </div>
                <div class="space-y-1">
                    <label class="text-xs font-medium text-red-700">Remove tags (one per line or comma-separated)</label>
                    <textarea class="input w-full h-28 resize-y font-mono text-xs"
                              placeholder="OldTag.To.Remove"
                              v-model="removeTagsInput"></textarea>
                </div>
            </div>
            <button @click="doEditTags" class="btn bg-blue-600 hover:bg-blue-700 text-white mb-2">Apply</button>
            <pre :class="{'text-red-600': out.edit.err}" class="out">{{ out.edit.text }}</pre>
        </div>
    </div>`,
};
