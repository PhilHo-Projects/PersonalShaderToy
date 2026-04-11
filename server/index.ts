import express from 'express';
import cors from 'cors';
import fs from 'fs';
import path from 'path';
import { WebSocketServer } from 'ws';
import { watch } from 'chokidar';
import shaderRoutes from './routes/shaders.js';

const PORT = 4781;
const SHADERS_DIR = path.resolve('shaders');
const DEFAULT_PROVIDERS = ['claude', 'gemini', 'openai'];

// Ensure default shader directories exist
for (const p of DEFAULT_PROVIDERS) {
  const dir = path.join(SHADERS_DIR, p);
  if (!fs.existsSync(dir)) fs.mkdirSync(dir, { recursive: true });
}

// Express
const app = express();
app.use(cors());
app.use(express.json());
app.use('/api', shaderRoutes);

const server = app.listen(PORT, () => {
  console.log(`Server running on http://localhost:${PORT}`);
});
server.on('error', (e: NodeJS.ErrnoException) => {
  if (e.code === 'EADDRINUSE') console.error(`Port ${PORT} in use — kill the other process or change PORT in server/index.ts`);
  else throw e;
});

// WebSocket — attach to Express server to share the same port
const wss = new WebSocketServer({ server });
console.log(`WebSocket attached to http://localhost:${PORT}`);

function broadcast(data: object) {
  const msg = JSON.stringify(data);
  for (const client of wss.clients) {
    if (client.readyState === 1) client.send(msg);
  }
}

// File watcher
const watcher = watch(SHADERS_DIR, {
  ignoreInitial: true,
  depth: 2,
});

watcher.on('add', (filePath) => {
  const rel = path.relative(SHADERS_DIR, filePath).replace(/\\/g, '/');
  const [provider, filename] = rel.split('/');
  if (provider && filename) {
    console.log(`File added: ${provider}/${filename}`);
    broadcast({ type: 'file-added', provider, filename });
  }
});

watcher.on('change', (filePath) => {
  const rel = path.relative(SHADERS_DIR, filePath).replace(/\\/g, '/');
  const [provider, filename] = rel.split('/');
  if (provider && filename) {
    broadcast({ type: 'file-changed', provider, filename });
  }
});

watcher.on('unlink', (filePath) => {
  const rel = path.relative(SHADERS_DIR, filePath).replace(/\\/g, '/');
  const [provider, filename] = rel.split('/');
  if (provider && filename) {
    broadcast({ type: 'file-removed', provider, filename });
  }
});
