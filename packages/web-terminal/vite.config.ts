import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import path from 'path';
import type { Plugin } from 'vite';
import fs from 'fs';

// Plugin to resolve bare .ts extension imports within workspace packages.
// Rollup doesn't auto-resolve .ts when following re-exports, so this plugin
// intercepts those resolution calls and appends .ts when needed.
function resolveWorkspaceTs(): Plugin {
  return {
    name: 'resolve-workspace-ts',
    resolveId(source, importer) {
      if (!importer) return null;
      if (!source.startsWith('.')) return null;
      if (!importer.includes('/packages/')) return null;
      if (!path.extname(source)) {
        const dir = path.dirname(importer);
        const tsPath = path.resolve(dir, source + '.ts');
        try {
          fs.accessSync(tsPath);
          return tsPath;
        } catch {
          return null;
        }
      }
      return null;
    },
  };
}

export default defineConfig({
  plugins: [
    resolveWorkspaceTs(),
    react({ tsconfig: './tsconfig.app.json' }),
  ],
  resolve: {
    extensions: ['.ts', '.tsx', '.js', '.jsx', '.json'],
    alias: {
      '@anomedge/contracts': path.resolve(__dirname, '../contracts/src/index.ts'),
      '@anomedge/bus':       path.resolve(__dirname, '../bus/src/index.ts'),
      '@anomedge/core':      path.resolve(__dirname, '../core/src/index.ts'),
    },
  },
  // public/ contains symlinks to:
  //   public/scenarios/*.json  → ../../scenarios/*.json
  //   public/policy/policy.yaml → ../../policy/policy.yaml
  publicDir: path.resolve(__dirname, 'public'),
  server: {
    port: 5173,
  },
});
