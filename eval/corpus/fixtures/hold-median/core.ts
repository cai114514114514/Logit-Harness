export function solve(data: (number|null)[]): number|null {
  if (!data.length) return null;
  const values = data.map(value => value ?? 0).sort();
  const middle = Math.floor(values.length / 2);
  return values.length % 2 ? values[middle] : (values[middle - 1] + values[middle]) / 2;
}
