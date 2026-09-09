<script setup lang="ts">
import type { MetadataNode } from '~/types/api'
import type { DraftMutations } from '~/utils/metadataModel'

/**
 * One additive-definition subtree. Nodes inherited from the saved predecessor
 * render read-only (additive-only editing); unsaved additions stay editable
 * until a revision is saved. All mutations and lookups go through the injected
 * draft mutator so recursive rendering stays flat.
 */
const props = withDefaults(defineProps<{
  nodes: MetadataNode[]
  path?: string
}>(), {
  path: '',
})

const mutator = inject<DraftMutations>('metadataDraftMutations')

/**
 * Stable v-for key per node object: keying by name would remount the input
 * on every keystroke (renaming changes the key) and silently drop focus.
 */
const nodeIds = new WeakMap<MetadataNode, number>()
let nextNodeId = 1

function nodeId(node: MetadataNode): number {
  let id = nodeIds.get(node)
  if (id === undefined) {
    id = nextNodeId++
    nodeIds.set(node, id)
  }
  return id
}

function childPath(node: MetadataNode): string {
  return props.path ? `${props.path}.${node.name}` : node.name
}

function nodeLocked(node: MetadataNode): boolean {
  return mutator?.isLocked(childPath(node)) ?? true
}

function removeLabel(node: MetadataNode): string {
  return node.kind === 'object'
    ? `Remove object ${childPath(node)} and its children`
    : `Remove field ${childPath(node)}`
}
</script>

<template>
  <ul class="flex flex-col gap-3" :class="path ? 'ml-6 border-l border-border pl-4' : ''">
    <li
      v-for="node in nodes"
      :key="nodeId(node)"
      class="rounded-lg border border-border bg-surface px-4 py-3"
      :class="{ 'opacity-80': nodeLocked(node) }"
    >
      <div class="flex flex-wrap items-start gap-3">
        <div class="w-56">
          <NxInput
            :model-value="node.name"
            :label="node.kind === 'object' ? 'Object name' : 'Field name'"
            :disabled="nodeLocked(node) || mutator?.busy() === true"
            :error="mutator?.violation(childPath(node))"
            :hint="nodeLocked(node) ? undefined : '1-64 characters, starting with a letter'"
            @update:model-value="mutator?.rename(node, $event)"
          />
        </div>
        <div v-if="node.kind === 'field'" class="w-48">
          <NxSelect
            :model-value="node.generator"
            label="Generator"
            :options="GENERATOR_CATALOGUE.map(entry => ({ value: entry.value, label: entry.label }))"
            :disabled="nodeLocked(node) || mutator?.busy() === true"
            @update:model-value="mutator?.setGenerator(node, $event)"
          />
        </div>
        <div class="flex flex-1 items-end justify-end gap-2">
          <NxBadge v-if="nodeLocked(node)" tone="neutral">Immutable</NxBadge>
          <NxButton
            v-else
            variant="ghost"
            size="sm"
            :disabled="mutator?.busy() === true"
            :aria-label="removeLabel(node)"
            @click="mutator?.remove(node)"
          >
            Remove
          </NxButton>
        </div>
      </div>
      <p v-if="node.kind === 'field'" class="mt-2 text-xs text-faint">
        {{ catalogueEntry(node.generator)?.constraint }} — sourced from
        {{ catalogueEntry(node.generator)?.source }}
      </p>

      <template v-if="node.kind === 'object'">
        <MetadataNodeEditor
          :nodes="node.children ?? []"
          :path="childPath(node)"
        />
        <div class="mt-2 flex gap-2">
          <NxButton
            variant="ghost"
            size="sm"
            :disabled="mutator?.busy() === true"
            @click="mutator?.addField(node)"
          >
            Add field
          </NxButton>
          <NxButton
            variant="ghost"
            size="sm"
            :disabled="mutator?.busy() === true"
            @click="mutator?.addObject(node)"
          >
            Add object
          </NxButton>
        </div>
      </template>
    </li>
  </ul>
</template>
