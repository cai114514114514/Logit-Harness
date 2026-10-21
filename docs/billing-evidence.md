# Billing evidence required to resume the study

Checked again on 2026-10-22 after the goal was resumed. The existing 50 RMB authorization and the
ledger are unchanged. This check made no model request and released no uncertain allowance.

## What the official sources establish

- [Model pricing](https://api-docs.deepseek.com/zh-cn/quick_start/pricing/) describes token-based
  model charges and separate peak/off-peak prices. The campaign deliberately retains its recorded
  conservative peak-rate schedule; the price page is not an invoice for these individual calls.
- [Model error codes](https://api-docs.deepseek.com/zh-cn/quick_start/error_codes/) describes HTTP
  400 as invalid request format, but does not state the billed amount of these failed Responses
  requests. No returned usage was saved for the 13 uncertain calls.
- [Search/Fetch billing](https://api-docs.deepseek.com/zh-cn/guides/search/) explicitly excludes
  failed/no-result requests for those APIs. This separate per-request policy cannot establish the
  cost of a Responses model request.
- [Get User Balance](https://api-docs.deepseek.com/api/get-user-balance/) returns current aggregate
  available balances. It does not provide the historical request-level charges needed here; no
  starting account-balance snapshot exists for attributing a later balance difference to this study.
- The official [FAQ](https://static.deepseek.com/faq/index.html?lang=zh) links the platform's
  [usage page](https://platform.deepseek.com/usage) and [billing page](https://platform.deepseek.com/transactions).

The public sources inspected do not justify silently treating the uncertain amount as zero.
This is an evidence limitation, not a claim that the provider actually charged the reserved maximum.

## Concrete next input

Opening the official usage page in the user's Chrome session redirected to `/sign_in`. The tab is
retained for the user to complete login. No password, verification code, login click or payment was
submitted by the agent. Alternatively, a local export covering **2026-10-22 02:23–03:35 Beijing time**
can support the same review without browser login.

The readonly reconciliation packet is `artifacts/campaign/billing-review.json`: twelve HTTP 400
provider request IDs plus one network-send failure without a provider request ID, grouped by the
five original sessions. The packet preserves the 13 ledger charge IDs and their maximum allowances;
it does not invent an exact foreign-key relationship between a charge and a provider request.

Any later reconciliation must retain the original ledger/history, cite the actual billing evidence,
and distinguish an accounted estimate from a provider-confirmed charge. Login alone is not evidence
that a charge is zero, and a resume instruction does not enlarge the 50 RMB limit. The remaining
505 registered trials and actual human review are still required for the complete research goal.
