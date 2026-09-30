module Shop
  module Pricing
    def self.tax(amount)
      amount * 0.2
    end

    def self.round_cents(amount)
      amount.round(2)
    end
  end
end
