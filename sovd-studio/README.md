# SOVD Studio

SOVD Studio is the React and TypeScript frontend for the VeloSOVD
distributed virtual vehicle demonstrator.

## Development

From the repository root:

```bash
cd sovd-studio
npm ci
npm run dev
```

The Vite development server runs at `http://127.0.0.1:5173`.

The frontend development proxy forwards `/api` requests to the host
SOVD proxy at `http://127.0.0.1:18010`. Start the backend and host
proxy using [the development guide](../docs/DEVELOPMENT.md).

## Production build

```bash
npm ci
npm run build
```

The production output is generated in `dist/`. This directory is build
output and is not committed to the repository.

## Technology

- React
- TypeScript
- Vite
