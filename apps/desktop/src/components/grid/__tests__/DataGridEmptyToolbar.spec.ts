import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

const dataGridSource = readFileSync(new URL("../DataGrid.vue", import.meta.url), "utf8");
const documentBrowserSource = readFileSync(new URL("../../document/DocumentBrowser.vue", import.meta.url), "utf8");

describe("empty data grid toolbar", () => {
  it("keeps the toolbar branch mounted when an empty result still exposes toolbar actions", () => {
    expect(dataGridSource).toContain('v-if="hasData || canShowWhereSearch || showDataGridTopbar"');
  });

  it("keeps empty MongoDB-only actions visible but disabled", () => {
    expect(dataGridSource).toContain('const showMongoJsonPreviewAction = computed(() => props.databaseType === "mongodb")');
    expect(dataGridSource).toContain('<Tooltip v-if="showMongoJsonPreviewAction">');
    expect(dataGridSource).toContain(':disabled="!canShowMongoJsonPreview"');
    expect(dataGridSource).toContain(':disabled="props.result.columns.length === 0"');
  });

  it("reloads the document browser when its data context changes", () => {
    expect(documentBrowserSource).toContain('() => [props.connectionId, props.database, props.collection, props.databaseType] as const');
    expect(documentBrowserSource).toContain('if (!documentBrowserMounted || context.every((value, index) => value === previousContext[index])) return;');
    expect(documentBrowserSource).toContain('void load({ page: 0, offset: 0 });');
  });
});
