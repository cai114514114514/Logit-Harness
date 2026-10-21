import readline from "node:readline";
import { solve } from "./core.ts";

const input = readline.createInterface({ input: process.stdin });
for await (const line of input) {
  const request = JSON.parse(line);
  const answer = request.op === "about" ? {name: "hold-median", version: 1} : solve(request.data);
  console.log(JSON.stringify(answer));
}
