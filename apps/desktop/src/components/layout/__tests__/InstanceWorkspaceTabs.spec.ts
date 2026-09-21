// @vitest-environment happy-dom

import { createApp, defineComponent, h, nextTick, type App } from "vue";
import { afterEach, describe, expect, it, vi } from "vitest";
import InstanceWorkspaceTabs from "../InstanceWorkspaceTabs.vue";
import { connectionIconType } from "@/lib/connection/connectionPresentation";
import type { ConnectionConfig, SidebarLayout } from "@/types/database";

vi.mock("@lucide/vue", () => ({
  X: defineComponent({
    name: "XIcon",
    setup: () => () => h("span", { "data-x-icon": "" }),
  }),
}));

vi.mock("@/components/icons/DatabaseIcon.vue", () => ({
  default: defineComponent({
    name: "DatabaseIconStub",
    props: {
      dbType: { type: String, default: undefined },
    },
    setup(props) {
      return () => h("span", { "data-db-type": props.dbType ?? "" });
    },
  }),
}));

const mountedApps: App[] = [];

const layout: SidebarLayout = {
  groups: [{ id: "g1", name: "Default", collapsed: false }],
  order: [{ type: "group", id: "g1", children: [] }],
};

function connection(partial: Partial<ConnectionConfig> & Pick<ConnectionConfig, "id" | "db_type">): ConnectionConfig {
  return {
    name: partial.id,
    host: "localhost",
    port: 5432,
    username: "u",
    password: "",
    ...partial,
  } as ConnectionConfig;
}

async function mountTabs(connections: ConnectionConfig[], activeConnectionId: string | null = connections[0]?.id ?? null) {
  const container = document.createElement("div");
  document.body.append(container);
  const app = createApp(InstanceWorkspaceTabs, {
    connections,
    layout,
    activeConnectionId,
  });
  mountedApps.push(app);
  app.mount(container);
  await nextTick();
  return container;
}

describe("InstanceWorkspaceTabs", () => {
  afterEach(() => {
    for (const app of mountedApps.splice(0)) app.unmount();
    document.body.innerHTML = "";
  });

  it("passes connectionIconType to DatabaseIcon for each workspace tab", async () => {
    const connections = [connection({ id: "mysql-1", db_type: "mysql" }), connection({ id: "pg-1", db_type: "postgres", driver_profile: "postgresql" })];
    const root = await mountTabs(connections);

    const icons = root.querySelectorAll("[data-db-type]");
    expect(icons).toHaveLength(2);
    expect(icons[0]?.getAttribute("data-db-type")).toBe(connectionIconType(connections[0]));
    expect(icons[1]?.getAttribute("data-db-type")).toBe(connectionIconType(connections[1]));
  });
});
