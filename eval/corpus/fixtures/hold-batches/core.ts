export function solve(data: any): number[][] {
  const batches: number[][] = []; let batch: number[] = [], weight = 0;
  for (const item of data.weights) {
    if (batch.length && weight + item > data.capacity) {batches.push(batch); batch = []; weight = 0;}
    batch.push(item); weight += item;
    if (item > data.capacity) {batches.push(batch); batch = []; weight = 0;}
  }
  batches.push(batch);
  return batches;
}
