import path from 'path';
import { fileURLToPath } from 'url';

const SERVER_DIR = path.dirname(fileURLToPath(import.meta.url));

export const SHADERS_DIR = path.resolve(SERVER_DIR, '../../../shaders');
