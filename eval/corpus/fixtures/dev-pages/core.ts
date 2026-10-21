export function solve(data: any): number[] {
  const rows = data.items.filter((value: number) => value >= data.cursor);
  return data.limit ? rows.slice(0, data.limit) : rows;
}
