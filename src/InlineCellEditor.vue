<script setup lang="ts">
import { GridEditing } from './useGridEditing';
import { computed } from 'vue';
defineOptions({
  inheritAttrs: false,
});
const props = defineProps<{
  editing: GridEditing;
  row: number;
  column: number;
}>();
const message = computed(() => props.editing.message);
const cell = computed(() => props.editing.cell!);
const preview = computed(() => props.editing.preview);
const busy = computed(() => props.editing.busy);
const data = computed(() => props.editing.data);
const rowOffset = computed(() => props.editing.rowOffset);
const cellError = computed(() => props.editing.cellError);
const engine = computed(() => props.editing.engine);
const submitted = computed(() => props.editing.submitted);
const onMove = computed(() => props.editing.onMove);
const close = props.editing.close;
const stage = props.editing.stage;
const setCell = props.editing.setCell;
const setCellError = props.editing.setCellError;
const apply = props.editing.apply;
const row = computed(() => props.row);
const column = computed(() => props.column);
</script>

<template>
  <div
    class="inline-cell-editor"
    @click="(event) => event.stopPropagation()"
    @dblclick="(event) => event.stopPropagation()"
    @contextmenu="(event) => event.stopPropagation()"
    @focusout="
      (event) => {
        if (
          !(event.currentTarget as HTMLInputElement).contains(
            event.relatedTarget as Node | null,
          )
        )
          stage();
      }
    "
  >
    <textarea
      :rows="1"
      autofocus
      aria-label="Cell value"
      :placeholder="cell.null ? 'NULL' : ''"
      :value="cell.text"
      @focus="
        (event) => {
          const _cell5 = cell!;
          if (!_cell5.replace)
            (event.currentTarget as HTMLInputElement).select();
          else
            (event.currentTarget as HTMLInputElement).setSelectionRange(
              _cell5.text.length,
              _cell5.text.length,
            );
        }
      "
      @input="
        (event) => {
          const _cell5 = cell!;
          return setCell({
            ..._cell5,
            text: (event.target as HTMLInputElement).value,
            null: false,
          });
        }
      "
      @keydown="
        (event) => {
          const _onMove = onMove!;
          event.stopPropagation();
          const td = (event.currentTarget as HTMLInputElement).closest('td')!;
          if ((event.ctrlKey || event.metaKey) && event.key === 'Delete' && !event.isComposing) {
            event.preventDefault();
            stage(null);
            td.focus();
          } else if (event.key === 'Escape') {
            event.preventDefault();
            props.editing.cancelInline();
            setCell(null);
            td.focus();
          } else if (
            event.key === 'Enter' &&
            !event.shiftKey &&
            !event.isComposing
          ) {
            event.preventDefault();
            stage();
            _onMove?.(row, column, 'ArrowDown');
          } else if (event.key === 'Tab') {
            event.preventDefault();
            stage();
            _onMove?.(row, column, 'Tab', event.shiftKey);
          }
        }
      "
    ></textarea>
  </div>
</template>
