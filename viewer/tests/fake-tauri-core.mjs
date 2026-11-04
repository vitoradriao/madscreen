// Substituto mínimo de @tauri-apps/api/core para testes em Node.
export function invoke() { throw new Error("invoke indisponível em teste"); }
export function isTauri() { return false; }
