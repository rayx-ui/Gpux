### Expected outcome

The standalone TextArea fixture should avoid SelectionArea adapter text and row-hit-map work, improving or maintaining p50 and p95 while preserving rendered scroll behavior. The SelectionArea fixture must continue to register and operate normally.

Reject the experiment if a focused fixture fails, a screenshot regresses, or the standalone TextArea p95 worsens by more than five percent.
