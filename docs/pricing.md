# Evaluation price snapshot — 2026-10-21

Source: https://api-docs.deepseek.com/zh-cn/quick_start/pricing/

The user authorized **50 RMB total**, including all agents and web calls. No model call has been
made at the time this snapshot was recorded. Use peak prices as a conservative upper-bound ledger:

| Resource | RMB rate |
|---|---:|
| Flash uncached input | 2 / million tokens |
| Flash cached input | 0.04 / million tokens |
| Flash output, including reasoning | 8 / million tokens |
| Direct Search | 0.005 / request |
| Direct Fetch | 0.005 / request |

Do not claim the temporary free server-side search promotion for our independently issued requests.
Count uncertain calls at their reserved upper bound. Report conservative accounted cost separately
from confirmed usage; do not label this estimate as the provider's actual invoice. Reverify this
snapshot before starting a later campaign. The model alias currently serves DeepSeek-V4.1-Flash-1017;
the provider advertises 1M context and maximum 384K output. Weft may use smaller operational limits.
