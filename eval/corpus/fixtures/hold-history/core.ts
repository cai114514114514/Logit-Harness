export function solve(data: any[]): any[] {
  let value: any = 0; const past: any[] = [], future: any[] = [], out: any[] = [];
  for (const op of data) {
    if (op[0] === "set") {past.push(value); value = op[1];}
    else if (op[0] === "undo") {future.push(value); value = past.pop();}
    else if (op[0] === "redo" && future.length) {past.push(value); value = future.pop();}
    else if (op[0] === "get") out.push(value);
  }
  return out;
}
