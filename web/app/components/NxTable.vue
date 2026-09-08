<script setup lang="ts" generic="T extends Record<string, unknown>">
export interface TableColumn {
  key: string
  label: string
}

defineProps<{
  columns: TableColumn[]
  items: T[]
}>()
</script>

<template>
  <div class="max-w-full overflow-x-auto rounded-lg border border-border bg-surface">
    <table class="w-full text-sm">
      <thead>
        <tr class="border-b border-border bg-surface-raised text-left">
          <th
            v-for="column in columns"
            :key="column.key"
            scope="col"
            class="px-4 py-3 font-medium text-muted"
          >
            {{ column.label }}
          </th>
        </tr>
      </thead>
      <tbody v-if="items.length > 0">
        <tr
          v-for="(item, index) in items"
          :key="index"
          class="border-b border-border transition-colors last:border-0 hover:bg-surface-raised/60"
        >
          <td v-for="column in columns" :key="column.key" class="px-4 py-3 align-top">
            <slot :name="`cell-${column.key}`" :item="item">
              {{ item[column.key] }}
            </slot>
          </td>
        </tr>
      </tbody>
      <tbody v-else>
        <tr>
          <td :colspan="columns.length" class="px-4 py-6">
            <slot name="empty" />
          </td>
        </tr>
      </tbody>
    </table>
  </div>
</template>
