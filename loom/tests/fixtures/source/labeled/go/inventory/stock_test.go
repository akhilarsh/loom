package inventory

import "testing"

func TestCount(t *testing.T) {
	s := NewStock()
	s.Add("a", 1)
	if s.Count() != 1 {
		t.Fatal("count")
	}
}
