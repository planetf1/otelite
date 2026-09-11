// Command palette (#213): ⌘K / Ctrl+K from any view, jump to a GenAI report.
//
// Results are the 28 analytics reports, matched by title, hint, group
// label and the REPORT_KEYWORDS vocabulary (the same map the filter box
// uses, so the palette and the filter agree). Selection reuses the #212
// deep link: it writes #/analytics?report=<id> and the App's hashchange
// (first render of the analytics view) or the view's own hashchange
// listener (view already rendered) applies the jump — expand, scroll,
// flash. Nothing here duplicates that machinery.

class CommandPalette {
    constructor(app) {
        this.app = app;
        this.open = false;
        this.selectedIndex = 0;
        this.entries = null;
        this.el = null;
        this.input = null;
        this.list = null;
        document.addEventListener('keydown', (e) => {
            if ((e.metaKey || e.ctrlKey) && !e.altKey && !e.shiftKey &&
                (e.key === 'k' || e.key === 'K')) {
                e.preventDefault();
                if (this.open) this.close(); else this.openPalette();
                return;
            }
            if (!this.open) return;
            if (e.key === 'Escape') {
                e.preventDefault();
                this.close();
            }
        });
    }

    _buildEntries() {
        if (this.entries) return this.entries;
        const AV = window.AnalyticsView;
        const entries = [];
        for (const g of AV.GROUPS) {
            for (const id of g.reports) {
                const r = AV.REPORTS.find(x => x.id === id);
                if (!r) continue;
                const kws = (AV.REPORT_KEYWORDS[id] || []).join(' ');
                entries.push({
                    id,
                    label: r.title,
                    group: g.label,
                    hint: r.hint,
                    hay: `${r.title} ${r.hint} ${g.label} ${kws}`.toLowerCase(),
                });
            }
        }
        this.entries = entries;
        return entries;
    }

    openPalette() {
        if (this.open) return;
        this.open = true;
        const overlay = document.createElement('div');
        overlay.className = 'cmd-palette-overlay';
        // Outside click closes (mousedown on the overlay itself only —
        // clicks inside the panel must not dismiss).
        overlay.addEventListener('mousedown', (e) => {
            if (e.target === overlay) this.close();
        });
        overlay.innerHTML = `
            <div class="cmd-palette" role="dialog" aria-label="Jump to report">
                <input type="search" class="cmd-palette-input"
                       placeholder="Jump to a report… (e.g. ttft, retry, skill)"
                       autocomplete="off" spellcheck="false">
                <div class="cmd-palette-list" role="listbox"></div>
                <div class="cmd-palette-foot">↑↓ navigate · enter jump · esc close</div>
            </div>
        `;
        this.el = overlay;
        this.input = overlay.querySelector('.cmd-palette-input');
        this.list = overlay.querySelector('.cmd-palette-list');
        this.input.addEventListener('input', () => this._render(this.input.value));
        this.input.addEventListener('keydown', (e) => {
            const n = this.list.children.length;
            if (e.key === 'ArrowDown') {
                e.preventDefault();
                this.selectedIndex = Math.min(this.selectedIndex + 1, Math.max(n - 1, 0));
                this._updateSelection();
            } else if (e.key === 'ArrowUp') {
                e.preventDefault();
                this.selectedIndex = Math.max(this.selectedIndex - 1, 0);
                this._updateSelection();
            } else if (e.key === 'Enter') {
                e.preventDefault();
                this._select(this.selectedIndex);
            }
        });
        document.body.appendChild(overlay);
        this._render('');
        this.input.focus();
    }

    _render(query) {
        const q = (query || '').trim().toLowerCase();
        const tokens = q ? q.split(/\s+/) : [];
        const entries = this._buildEntries().filter(en =>
            tokens.length === 0 || tokens.every(t => en.hay.includes(t))
        );
        const shown = entries.slice(0, 12);
        this._results = shown;
        this.selectedIndex = 0;
        this.list.innerHTML = shown.length === 0
            ? '<div class="cmd-palette-empty">No matching reports</div>'
            : shown.map((en, i) => `
                <div class="cmd-palette-item" role="option" data-idx="${i}">
                    <span class="cmd-palette-label">${this._esc(en.label)}</span>
                    <span class="cmd-palette-group">${this._esc(en.group)}</span>
                </div>`).join('');
        this._updateSelection();
    }

    _updateSelection() {
        [...this.list.children].forEach((el, i) => {
            el.classList.toggle('cmd-palette-selected', i === this.selectedIndex);
        });
    }

    _select(i) {
        const en = this._results[i];
        if (!en) return;
        this.close();
        // Merge into the current hash when arriving from another view
        // (carries filter dimensions across, e.g. #/sessions?agent=…);
        // drop a stale report param from a non-analytics hash.
        const hash = window.location.hash || '';
        const path = hash.split('?')[0];
        const params = new URLSearchParams(
            hash.includes('?') ? hash.slice(hash.indexOf('?') + 1) : ''
        );
        if (path !== '#/analytics') params.delete('report');
        params.set('report', en.id);
        window.location.hash = `#/analytics?${params.toString()}`;
    }

    close() {
        if (!this.open) return;
        this.open = false;
        if (this.el) this.el.remove();
        this.el = null;
        this.input = null;
        this.list = null;
        this.selectedIndex = 0;
    }

    _esc(s) {
        const d = document.createElement('div');
        d.textContent = String(s ?? '');
        return d.innerHTML;
    }
}

window.CommandPalette = CommandPalette;
export { CommandPalette };
