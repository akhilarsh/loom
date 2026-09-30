package pricing

import "fmt"

const Currency = "EUR"

type Line struct {
	Name  string
	Price float64
}

func Total(lines []Line) float64 {
	sum := 0.0
	for _, l := range lines {
		sum += applyTax(l.Price)
	}
	return sum
}

func Format(amount float64) string {
	return fmt.Sprintf("%.2f %s", amount, Currency)
}
