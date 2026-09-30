package inventory

import price "example.com/shop/pricing"

// Label forwards to pricing.Format for callers that import only inventory.
func Label(amount float64) string {
	return price.Format(amount)
}
