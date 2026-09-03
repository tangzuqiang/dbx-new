<script setup lang="ts">
import { X } from "@lucide/vue";
import DatabaseIcon from "@/components/icons/DatabaseIcon.vue";
import { instanceWorkspaceTitle } from "@/lib/workspace/instanceWorkspace";
import type { ConnectionConfig, SidebarLayout } from "@/types/database";

defineProps<{
  connections: ConnectionConfig[];
  layout: SidebarLayout;
  activeConnectionId: string | null;
}>();

const emit = defineEmits<{
  activate: [connectionId: string];
  close: [connectionId: string];
}>();
</script>

<template>
  <div v-if="connections.length" class="flex h-9 min-w-0 shrink-0 items-end gap-1 overflow-x-auto border-b bg-muted/45 px-2 pt-1" data-instance-workspace-tabs>
    <button
      v-for="connection in connections"
      :key="connection.id"
      type="button"
      class="group flex h-8 min-w-32 max-w-64 shrink-0 items-center gap-1.5 rounded-t-md border border-b-0 px-2 text-xs transition-colors"
      :class="connection.id === activeConnectionId ? 'border-border bg-background text-foreground font-medium' : 'border-transparent text-muted-foreground hover:bg-background/60 hover:text-foreground'"
      :title="instanceWorkspaceTitle(connection, layout)"
      :data-active-workspace="connection.id === activeConnectionId"
      @click="emit('activate', connection.id)"
      @mousedown.middle.prevent="emit('close', connection.id)"
    >
      <DatabaseIcon :type="connection.driver_profile || connection.db_type" class="h-3.5 w-3.5 shrink-0" />
      <span class="min-w-0 flex-1 truncate">{{ instanceWorkspaceTitle(connection, layout) }}</span>
      <span
        role="button"
        tabindex="0"
        class="flex h-5 w-5 shrink-0 items-center justify-center rounded opacity-0 hover:bg-muted group-hover:opacity-100"
        aria-label="关闭工作区"
        @click.stop="emit('close', connection.id)"
        @keydown.enter.stop="emit('close', connection.id)"
      >
        <X class="h-3 w-3" />
      </span>
    </button>
  </div>
</template>
