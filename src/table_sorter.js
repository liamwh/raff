document.addEventListener('DOMContentLoaded', function() {
    const getCellValue = (tr, idx, type) => {
        const cellContent = tr.children[idx].innerText || tr.children[idx].textContent;
        if (type === 'number') {
            const num = parseFloat(cellContent.replace(/[%$,]/g, ''));
            return isNaN(num) ? -Infinity : num;
        }
        return cellContent.trim().toLowerCase();
    };

    const comparer = (idx, asc, type) => (a, b) => {
        const vA = getCellValue(a, idx, type);
        const vB = getCellValue(b, idx, type);
        let comparison = 0;
        if (type === 'number') {
            comparison = vA - vB;
        } else {
            comparison = vA.toString().localeCompare(vB.toString());
        }
        return asc ? comparison : -comparison;
    };

    document.querySelectorAll('.sortable-table .sortable-header').forEach(th => {
        th.addEventListener('click', (() => {
            const table = th.closest('table');
            const tbody = table.querySelector('tbody');
            if (!tbody) return;
            const columnIndex = parseInt(th.dataset.columnIndex);
            const sortType = th.dataset.sortType || 'string';
            let currentAsc = th.classList.contains('sort-asc');
            let newAsc;

            table.querySelectorAll('.sortable-header').forEach(otherTh => {
                if (otherTh === th) {
                    if (th.dataset.sortDirection && th.dataset.sortDirection !== 'none') {
                        newAsc = !currentAsc;
                        th.dataset.sortDirection = newAsc ? 'asc' : 'desc';
                    } else {
                        newAsc = true;
                        th.dataset.sortDirection = 'asc';
                    }
                    th.classList.toggle('sort-asc', newAsc);
                    th.classList.toggle('sort-desc', !newAsc);
                } else {
                    otherTh.classList.remove('sort-asc', 'sort-desc');
                    otherTh.dataset.sortDirection = 'none';
                }
            });
            if (newAsc === undefined) { // Should not happen if th is the clicked element
                newAsc = true;
                th.dataset.sortDirection = 'asc';
                th.classList.add('sort-asc');
            }

            Array.from(tbody.querySelectorAll('tr'))
                .sort(comparer(columnIndex, newAsc, sortType))
                .forEach(tr => tbody.appendChild(tr));
        }));
    });
});