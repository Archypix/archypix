const TaggingServicesTab = {
    props: ['state'],
    emits: ['update'],

    data(){
        return {
            services: [],
            create: {serviceType: 'rule', requires: '', excludes: ''},
            // per-service add-rule forms keyed by service id
            addForms: {},
            out: {
                list: {text: '', err: false},
                create: {text: '', err: false},
                reorder: {text: '', err: false},
            },
            // per-service output keyed by service id
            svcOut: {},
            // drag-and-drop reorder state
            dragIndex: null,
        };
    },

    computed: {
        typeBadgeClass(){
            return type => ({
                shared_tag_mapping: 'bg-purple-100 text-purple-800',
                rule: 'bg-blue-100   text-blue-800',
                segmentation: 'bg-orange-100 text-orange-800',
            }[type] || 'bg-gray-100 text-gray-600');
        },
        typeLabel(){
            return type => ({
                shared_tag_mapping: 'SharedTagMapping',
                rule: 'Rule',
                segmentation: 'Segmentation',
            }[type] || type);
        },
        // Services that can be reordered (Rule and Segmentation, sorted by current position).
        reorderableServices(){
            return this.services
                .filter(s => s.service_type !== 'shared_tag_mapping')
                .slice()
                .sort((a, b) => a.position - b.position || a.created_at.localeCompare(b.created_at));
        },
    },

    methods: {
        show(key, data, isErr = false){
            this.out[key] = {
                text: isErr ? `❌ ${data}` : (typeof data === 'string' ? data : JSON.stringify(data, null, 2)),
                err: isErr,
            };
        },

        showSvc(id, data, isErr = false){
            this.svcOut[id] = {
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

        parseTags(input){
            return input.split(/[\n,]+/).map(t => t.trim()).filter(Boolean);
        },

        // datetime-local gives "YYYY-MM-DDTHH:MM", backend needs seconds
        toDateTime(v){
            if(!v) return null;
            return v.length === 16 ? v + ':00' : v;
        },

        initForm(svc){
            if(this.addForms[svc.id]) return;
            const base = {incomingShareId: '', assignTag: '', predicate: '', name: '', dateStart: '', dateEnd: '', parentSegmentId: ''};
            this.addForms = {...this.addForms, [svc.id]: {...base}};
        },

        async doList(){
            const r = await this.api('/api/authenticated/tagging-services');
            if(r.ok){
                this.services = r.data;
                for(const svc of r.data) this.initForm(svc);
                this.show('list', `Loaded ${r.data.length} service(s).`);
            }else{
                this.show('list', r.data, true);
            }
        },

        async doCreate(){
            const r = await this.api('/api/authenticated/tagging-services', {
                method: 'POST',
                body: JSON.stringify({
                    service_type: this.create.serviceType,
                    requires: this.parseTags(this.create.requires),
                    excludes: this.parseTags(this.create.excludes),
                }),
            });
            this.show('create', r.data, !r.ok);
            if(r.ok) this.doList();
        },

        async doToggleEnabled(svc){
            const r = await this.api(`/api/authenticated/tagging-services/${svc.id}`, {
                method: 'PATCH',
                body: JSON.stringify({enabled: !svc.enabled}),
            });
            this.showSvc(svc.id, r.data, !r.ok);
            if(r.ok) this.doList();
        },

        async doDelete(svcId, promote = false){
            if(!confirm('Delete this service and all its rules?')) return;
            const r = await this.api(`/api/authenticated/tagging-services/${svcId}?promote_tags=${promote}`, {method: 'DELETE'});
            this.showSvc(svcId, r.data, !r.ok);
            if(r.ok) this.doList();
        },

        // ── Mappings (SharedTagMapping) ──────────────────────────────────────

        async doAddMapping(svc){
            const f = this.addForms[svc.id];
            const r = await this.api(`/api/authenticated/tagging-services/${svc.id}/mappings`, {
                method: 'POST',
                body: JSON.stringify({incoming_share_id: f.incomingShareId, assign_tag: f.assignTag}),
            });
            this.showSvc(svc.id, r.data, !r.ok);
            if(r.ok){
                f.incomingShareId = '';
                f.assignTag = '';
                this.doList();
            }
        },

        async doDeleteMapping(svc, ruleId){
            const r = await this.api(`/api/authenticated/tagging-services/${svc.id}/mappings/${ruleId}`, {method: 'DELETE'});
            this.showSvc(svc.id, r.data, !r.ok);
            if(r.ok) this.doList();
        },

        // ── Rules (Rule) ─────────────────────────────────────────────────────

        async doAddRule(svc){
            const f = this.addForms[svc.id];
            const r = await this.api(`/api/authenticated/tagging-services/${svc.id}/rules`, {
                method: 'POST',
                body: JSON.stringify({predicate: f.predicate, assign_tag: f.assignTag}),
            });
            this.showSvc(svc.id, r.data, !r.ok);
            if(r.ok){
                f.predicate = '';
                f.assignTag = '';
                this.doList();
            }
        },

        async doDeleteRule(svc, ruleId){
            const r = await this.api(`/api/authenticated/tagging-services/${svc.id}/rules/${ruleId}`, {method: 'DELETE'});
            this.showSvc(svc.id, r.data, !r.ok);
            if(r.ok) this.doList();
        },

        // ── Segments (Segmentation) ──────────────────────────────────────────

        async doAddSegment(svc){
            const f = this.addForms[svc.id];
            const r = await this.api(`/api/authenticated/tagging-services/${svc.id}/segments`, {
                method: 'POST',
                body: JSON.stringify({
                    name: f.name,
                    date_start: this.toDateTime(f.dateStart),
                    date_end: this.toDateTime(f.dateEnd),
                    assign_tag: f.assignTag,
                    parent_segment_id: f.parentSegmentId || null,
                }),
            });
            this.showSvc(svc.id, r.data, !r.ok);
            if(r.ok){
                f.name = '';
                f.dateStart = '';
                f.dateEnd = '';
                f.assignTag = '';
                f.parentSegmentId = '';
                this.doList();
            }
        },

        async doDeleteSegment(svc, segId){
            const r = await this.api(`/api/authenticated/tagging-services/${svc.id}/segments/${segId}`, {method: 'DELETE'});
            this.showSvc(svc.id, r.data, !r.ok);
            if(r.ok) this.doList();
        },

        // ── Reorder (drag-and-drop) ──────────────────────────────────────────
        // Operates on a local sorted copy; sends the new order to the server.
        onDragStart(index){
            this.dragIndex = index;
        },
        onDragOver(e, index){
            e.preventDefault();
            if(this.dragIndex === null || this.dragIndex === index) return;
            // Reorder local sorted list.
            const list = this.reorderableServices.slice();
            const [moved] = list.splice(this.dragIndex, 1);
            list.splice(index, 0, moved);
            // Patch positions on the local services array so Vue re-renders.
            list.forEach((svc, i) => {
                const s = this.services.find(x => x.id === svc.id);
                if(s) s.position = i;
            });
            this.dragIndex = index;
        },
        onDragEnd(){
            this.dragIndex = null;
        },
        async doReorder(){
            const ids = this.reorderableServices.map(s => s.id);
            const r = await this.api('/api/authenticated/tagging-services/reorder', {
                method: 'POST',
                body: JSON.stringify({ordered_ids: ids}),
            });
            this.out.reorder = {
                text: r.ok ? `Order saved.` : `❌ ${JSON.stringify(r.data)}`,
                err: !r.ok,
            };
            if(r.ok) this.doList();
        },

        fmtDate(dt){
            return dt ? dt.replace('T', ' ').slice(0, 16) : '';
        },
    },

    template: `
    <div class="space-y-4">

        <!-- Create -->
        <div class="card">
            <h2 class="font-bold text-base mb-3 border-b pb-2">Create Tagging Service</h2>
            <div class="grid grid-cols-1 md:grid-cols-3 gap-2 mb-3">
                <div class="space-y-1">
                    <label class="text-xs font-medium text-gray-600">Type</label>
                    <select class="input w-full" v-model="create.serviceType">
                        <option value="rule">Rule</option>
                        <option value="segmentation">Segmentation</option>
                        <option value="shared_tag_mapping">SharedTagMapping</option>
                    </select>
                </div>
                <div class="space-y-1">
                    <label class="text-xs font-medium text-gray-600">Requires (comma-separated tag paths)</label>
                    <input class="input w-full" placeholder="Photos, Images" v-model="create.requires"/>
                </div>
                <div class="space-y-1">
                    <label class="text-xs font-medium text-gray-600">Excludes (comma-separated tag paths)</label>
                    <input class="input w-full" placeholder="Archive" v-model="create.excludes"/>
                </div>
            </div>
            <button @click="doCreate" class="btn bg-blue-600 hover:bg-blue-700 text-white mb-2">Create</button>
            <pre v-if="out.create.text" :class="{'text-red-600': out.create.err}" class="out">{{ out.create.text }}</pre>
        </div>

        <!-- Reorder -->
        <div class="card" v-if="reorderableServices.length > 1">
            <h2 class="font-bold text-base mb-3 border-b pb-2">Pipeline Order
                <span class="font-normal text-xs text-gray-400 ml-1">(SharedTagMapping always runs first — drag to reorder Rule/Segmentation)</span>
            </h2>
            <div class="space-y-1 mb-3">
                <div v-for="(svc, i) in reorderableServices" :key="svc.id"
                     draggable="true"
                     @dragstart="onDragStart(i)"
                     @dragover="onDragOver($event, i)"
                     @dragend="onDragEnd"
                     :class="dragIndex === i ? 'opacity-40' : ''"
                     class="flex items-center gap-2 px-3 py-1.5 border rounded bg-white cursor-grab select-none">
                    <span class="text-gray-300 text-sm">⠿</span>
                    <span class="text-xs text-gray-400 w-5 shrink-0">{{ i + 1 }}</span>
                    <span :class="typeBadgeClass(svc.service_type)"
                          class="text-xs font-semibold px-2 py-0.5 rounded-full shrink-0">{{ typeLabel(svc.service_type) }}</span>
                    <span class="font-mono text-xs text-gray-400 truncate flex-1">{{ svc.id }}</span>
                    <span v-if="svc.requires && svc.requires.length" class="text-xs text-gray-400 shrink-0">needs: {{ svc.requires.join(', ') }}</span>
                </div>
            </div>
            <button @click="doReorder" class="btn bg-indigo-600 hover:bg-indigo-700 text-white text-sm mb-2">Save Order</button>
            <pre v-if="out.reorder.text" :class="{'text-red-600': out.reorder.err}" class="out">{{ out.reorder.text }}</pre>
        </div>

        <!-- List -->
        <div class="card">
            <div class="flex items-center gap-3 mb-3 border-b pb-2">
                <h2 class="font-bold text-base flex-1">Tagging Services</h2>
                <button @click="doList" class="btn bg-gray-200 hover:bg-gray-300">Refresh</button>
            </div>
            <pre v-if="out.list.text" :class="{'text-red-600': out.list.err}" class="out mb-3">{{ out.list.text }}</pre>
            <div v-if="services.length === 0" class="text-xs text-gray-400">No services. Click Refresh to load.</div>

            <div v-for="svc in services" :key="svc.id" class="border rounded mb-4 overflow-hidden">

                <!-- Service header -->
                <div class="flex items-center gap-2 px-3 py-2 bg-gray-50 border-b">
                    <span :class="typeBadgeClass(svc.service_type)"
                          class="text-xs font-semibold px-2 py-0.5 rounded-full shrink-0">{{ typeLabel(svc.service_type) }}</span>
                    <span v-if="svc.service_type !== 'shared_tag_mapping'"
                          class="text-xs text-gray-400 shrink-0">#{{ svc.position }}</span>
                    <span class="font-mono text-xs text-gray-400 truncate flex-1">{{ svc.id }}</span>
                    <span v-if="svc.requires && svc.requires.length"
                          class="text-xs text-gray-500 shrink-0">needs: {{ svc.requires.join(', ') }}</span>
                    <span v-if="svc.excludes && svc.excludes.length"
                          class="text-xs text-gray-500 shrink-0">excl: {{ svc.excludes.join(', ') }}</span>
                    <button @click="doToggleEnabled(svc)"
                            :class="svc.enabled ? 'bg-green-100 text-green-800 hover:bg-green-200' : 'bg-gray-100 text-gray-500 hover:bg-gray-200'"
                            class="btn text-xs py-0.5 shrink-0">{{ svc.enabled ? 'Enabled' : 'Disabled' }}</button>
                    <button @click="doDelete(svc.id)"
                            class="btn bg-red-100 hover:bg-red-200 text-red-700 text-xs py-0.5 shrink-0">Delete</button>
                    <button @click="doDelete(svc.id, true)"
                            class="btn bg-red-100 hover:bg-red-200 text-red-700 text-xs py-0.5 shrink-0">Delete (Promote)</button>
                </div>

                <div class="p-3 space-y-3">

                    <!-- SharedTagMapping rules -->
                    <template v-if="svc.service_type === 'shared_tag_mapping'">
                        <div v-if="svc.mappings && svc.mappings.length" class="space-y-1">
                            <div v-for="m in svc.mappings" :key="m.id"
                                 class="flex items-center gap-2 text-xs border rounded px-2 py-1">
                                <span class="font-mono text-gray-400 truncate w-48 shrink-0" :title="m.incoming_share_id">{{ m.incoming_share_id }}</span>
                                <span class="text-gray-400">→</span>
                                <span class="font-medium flex-1 truncate">{{ m.assign_tag }}</span>
                                <span v-if="m.is_broken" class="text-red-500 shrink-0">⚠ broken</span>
                                <button @click="doDeleteMapping(svc, m.id)"
                                        class="btn bg-red-100 hover:bg-red-200 text-red-700 py-0 shrink-0">✕</button>
                            </div>
                        </div>
                        <div v-else class="text-xs text-gray-400">No mappings.</div>
                        <div v-if="addForms[svc.id]" class="flex gap-2 flex-wrap">
                            <input class="input flex-1 min-w-40" placeholder="incoming_share_id (UUID)"
                                   v-model="addForms[svc.id].incomingShareId"/>
                            <input class="input flex-1 min-w-32" placeholder="assign_tag (Photos.Friends.Bob)"
                                   v-model="addForms[svc.id].assignTag"/>
                            <button @click="doAddMapping(svc)"
                                    class="btn bg-blue-600 hover:bg-blue-700 text-white shrink-0">Add Mapping</button>
                        </div>
                    </template>

                    <!-- Rule tagging rules -->
                    <template v-else-if="svc.service_type === 'rule'">
                        <div v-if="svc.rules && svc.rules.length" class="space-y-1">
                            <div v-for="rule in svc.rules" :key="rule.id"
                                 class="flex items-center gap-2 text-xs border rounded px-2 py-1">
                                <span class="font-mono flex-1 truncate">{{ rule.predicate }}</span>
                                <span class="text-gray-400">→</span>
                                <span class="font-medium shrink-0">{{ rule.assign_tag }}</span>
                                <button @click="doDeleteRule(svc, rule.id)"
                                        class="btn bg-red-100 hover:bg-red-200 text-red-700 py-0 shrink-0">✕</button>
                            </div>
                        </div>
                        <div v-else class="text-xs text-gray-400">No rules.</div>
                        <div v-if="addForms[svc.id]" class="flex gap-2 flex-wrap">
                            <input class="input flex-1 min-w-48" placeholder='predicate (e.g. exif.gps within bbox(...))'
                                   v-model="addForms[svc.id].predicate"/>
                            <input class="input flex-1 min-w-32" placeholder="assign_tag (Photos.Places.Chamonix)"
                                   v-model="addForms[svc.id].assignTag"/>
                            <button @click="doAddRule(svc)"
                                    class="btn bg-blue-600 hover:bg-blue-700 text-white shrink-0">Add Rule</button>
                        </div>
                    </template>

                    <!-- Segmentation segments -->
                    <template v-else-if="svc.service_type === 'segmentation'">
                        <div v-if="svc.segments && svc.segments.length" class="space-y-1">
                            <div v-for="seg in svc.segments" :key="seg.id"
                                 class="flex items-center gap-2 text-xs border rounded px-2 py-1">
                                <span v-if="seg.parent_segment_id" class="text-gray-300 shrink-0">↳</span>
                                <span class="font-medium shrink-0">{{ seg.name }}</span>
                                <span class="text-gray-400 shrink-0">{{ fmtDate(seg.date_start) }} – {{ fmtDate(seg.date_end) }}</span>
                                <span class="text-gray-400">→</span>
                                <span class="flex-1 truncate">{{ seg.assign_tag }}</span>
                                <button @click="doDeleteSegment(svc, seg.id)"
                                        class="btn bg-red-100 hover:bg-red-200 text-red-700 py-0 shrink-0">✕</button>
                            </div>
                        </div>
                        <div v-else class="text-xs text-gray-400">No segments.</div>
                        <div v-if="addForms[svc.id]" class="grid grid-cols-2 md:grid-cols-3 gap-2">
                            <input class="input" placeholder="name (e.g. Alps trip)"
                                   v-model="addForms[svc.id].name"/>
                            <input class="input" type="datetime-local"
                                   v-model="addForms[svc.id].dateStart"/>
                            <input class="input" type="datetime-local"
                                   v-model="addForms[svc.id].dateEnd"/>
                            <input class="input" placeholder="assign_tag (Photos.Travel.Alps)"
                                   v-model="addForms[svc.id].assignTag"/>
                            <input class="input font-mono text-xs" placeholder="parent_segment_id (UUID, optional)"
                                   v-model="addForms[svc.id].parentSegmentId"/>
                            <button @click="doAddSegment(svc)"
                                    class="btn bg-blue-600 hover:bg-blue-700 text-white">Add Segment</button>
                        </div>
                    </template>

                    <!-- Per-service output -->
                    <pre v-if="svcOut[svc.id] && svcOut[svc.id].text"
                         :class="{'text-red-600': svcOut[svc.id].err}"
                         class="out">{{ svcOut[svc.id].text }}</pre>
                </div>
            </div>
        </div>
    </div>`,
};
