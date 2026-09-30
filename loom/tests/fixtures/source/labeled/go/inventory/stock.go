package inventory

import (
	"math"

	"example.com/shop/pricing"
)

type Saver interface {
	Save(item string) error
}

type Stock struct {
	items map[string]int
}

type Ledger struct {
	entries []string
}

func NewStock() *Stock {
	return &Stock{items: map[string]int{}}
}

func (s *Stock) Add(name string, qty int) {
	s.items[name] += qty
}

func (s *Stock) Count() int {
	return len(s.items)
}

func (s *Stock) Value() float64 {
	lines := []pricing.Line{{Name: "all", Price: 1.5}}
	return round(pricing.Total(lines))
}

func (s *Stock) Save(item string) error {
	s.Add(item, 1)
	return nil
}

func (l *Ledger) Save(item string) error {
	l.entries = append(l.entries, item)
	return nil
}

func Persist(dest Saver, item string) error {
	return dest.Save(item)
}

func round(v float64) float64 {
	return math.Floor(v)
}
