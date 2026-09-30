<?php
namespace Outer\Inner {
    class Widget {
        public function run() {
            return self::build();
        }

        public static function build() {}
    }
}
