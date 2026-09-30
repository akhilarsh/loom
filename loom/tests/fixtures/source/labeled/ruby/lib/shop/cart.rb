require_relative 'pricing'
require 'json'

module Shop
  class Cart
    attr_reader :items

    def initialize
      @items = []
    end

    def add(item)
      @items.push(item)
      self.touch
    end

    def total
      Pricing.tax(subtotal())
    end

    def subtotal
      @items.sum
    end

    def checkout(gateway)
      gateway.charge(total())
    end

    def to_json
      JSON.generate(@items)
    end
  end
end
