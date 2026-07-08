import type { ExportFormat } from "../../lib/types";

/** Filename for a collection- or request-export artifact, per format. */
export function exportFilename(format: ExportFormat, baseName: string): string {
  switch (format) {
    case "postman":
      return `${baseName}.postman_collection.json`;
    case "open_api":
      return `${baseName}.openapi.json`;
    case "har":
      return `${baseName}.har`;
    case "curl":
      return `${baseName}.sh`;
  }
}
