require_relative '../lib/shop/cart'

def test_total
  cart = Shop::Cart.new
  cart.add(1)
  cart.total()
end
