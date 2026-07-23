declare module '*.css';

interface ImportMetaEnv {
  readonly A3S_ARCHITECTURE_BASE_PATH?: string;
  readonly BASE_URL: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}
