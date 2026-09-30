module Outer
  module Inner
    class Widget
      def run(target)
        self.prepare
        finish()
        target.call
        Helper.build
      end

      def prepare; end

      def finish; end
    end
  end
end

class Outer::Inner::Deep
  def value; end
end
