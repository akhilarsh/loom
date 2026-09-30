package pricing

import "math"

const rate = 0.2

func applyTax(amount float64) float64 {
	return round(amount * (1 + rate))
}

func round(v float64) float64 {
	return math.Round(v*100) / 100
}
