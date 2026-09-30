module Shop
  class Cart
    def touch
      @touched = true
    end

    def summary
      self.total
    end
  end
end
