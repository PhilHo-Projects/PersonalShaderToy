import { Router, Request, Response } from 'express';
import fs from 'fs';
import path from 'path';
import { SHADERS_DIR } from '../shaderRoot.js';

const router = Router();
const EXTENSIONS = ['.glsl', '.frag', '.wgsl', '.hlsl'];

function getProviders(): string[] {
  if (!fs.existsSync(SHADERS_DIR)) return [];
  return fs.readdirSync(SHADERS_DIR, { withFileTypes: true })
    .filter(d => d.isDirectory() && !d.name.startsWith('.'))
    .map(d => d.name);
}

function getShaderType(filename: string): string {
  const ext = path.extname(filename).toLowerCase();
  if (ext === '.wgsl') return 'wgsl';
  if (ext === '.hlsl') return 'hlsl';
  return 'glsl';
}

function listShaderFiles(provider: string) {
  const dir = path.join(SHADERS_DIR, provider);
  if (!fs.existsSync(dir)) return [];
  return fs.readdirSync(dir)
    .filter(f => EXTENSIONS.includes(path.extname(f).toLowerCase()))
    .map(f => {
      const stat = fs.statSync(path.join(dir, f));
      return {
        name: f,
        provider,
        type: getShaderType(f),
        modified: stat.mtimeMs,
      };
    })
    .sort((a, b) => b.modified - a.modified);
}

router.get('/shaders', (_req: Request, res: Response) => {
  const result: Record<string, ReturnType<typeof listShaderFiles>> = {};
  for (const p of getProviders()) {
    result[p] = listShaderFiles(p);
  }
  res.json(result);
});

router.get('/shaders/:provider/:filename', (req: Request, res: Response) => {
  const provider = req.params.provider as string;
  const filename = req.params.filename as string;
  if (!getProviders().includes(provider)) {
    res.status(400).json({ error: 'Invalid provider' });
    return;
  }
  const filePath = path.join(SHADERS_DIR, provider, filename);
  if (!fs.existsSync(filePath)) {
    res.status(404).json({ error: 'File not found' });
    return;
  }
  const content = fs.readFileSync(filePath, 'utf-8');
  const stat = fs.statSync(filePath);
  res.json({
    name: filename,
    provider,
    type: getShaderType(filename),
    content,
    modified: stat.mtimeMs,
  });
});

router.post('/shaders/:provider/:filename', (req: Request, res: Response) => {
  const provider = req.params.provider as string;
  const filename = req.params.filename as string;
  // Let new providers be created dynamically if they don't exist
  // by removing the strict check for existing providers in the POST route
  const dir = path.join(SHADERS_DIR, provider);
  if (!fs.existsSync(dir)) fs.mkdirSync(dir, { recursive: true });
  const body = req.body as { content?: string };
  fs.writeFileSync(path.join(dir, filename), body.content || '', 'utf-8');
  res.json({ success: true });
});

export default router;
