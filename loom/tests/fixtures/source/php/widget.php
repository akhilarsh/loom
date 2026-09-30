<?php
use App\Support\Helper as H;
use function App\Support\helper_fn;

trait Loggable {
    public function log($message) {
        return $this->format($message);
    }

    private function format($message) {
        return $message;
    }
}

interface Runnable {
    public function run($target);
}

class Widget implements Runnable {
    use Loggable;

    public function run($target) {
        $this->prepare();
        self::build();
        static::make();
        $target->go();
        Helper::create();
        H::other();
        helper_fn();
        $name = 'x';
        $name();
    }

    public function prepare() {}

    public static function build() {}

    public static function make() {}
}
