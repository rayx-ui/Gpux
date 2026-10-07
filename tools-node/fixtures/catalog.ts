import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

/** A lane-B conformance scene, as `gpux-testkit web build` lists it in `catalog.json`. */
export interface CatalogScene {
  id: string;
  scales: number[];
  areas: string[];
}

/** The served build: `GPUX_DIST` from the testkit, or the default build folder. */
export const dist =
  process.env.GPUX_DIST ??
  join(dirname(fileURLToPath(import.meta.url)), '..', '..', 'target', 'gpux-web', 'dist');

/** Every lane-B scene of the build. Specs enumerate it at collection time. */
export function laneBScenes(): CatalogScene[] {
  try {
    return JSON.parse(readFileSync(join(dist, 'catalog.json'), 'utf8'));
  } catch (error) {
    throw new Error(
      `no catalog.json in ${dist}: run \`gpux-testkit web build\` first (${String(error)})`,
    );
  }
}
