export function solve(data: any): boolean {
  const rule = data.rules.find((rule: any) => data.path.startsWith(rule.prefix));
  return rule ? rule.allow : true;
}
