package main

import (
	"fmt"

	"example.com/shop/inventory"
	price "example.com/shop/pricing"
)

func main() {
	stock := inventory.NewStock()
	stock.Add("bolt", 3)
	_ = inventory.Persist(stock, "bolt")
	fmt.Println(stock.Value(), price.Format(2.5), inventory.Label(1))
}
