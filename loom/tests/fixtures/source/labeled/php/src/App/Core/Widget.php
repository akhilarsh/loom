<?php
namespace App\Core;

use App\Contracts\Saver as SaverContract;
use App\Support\{Loggable, Strings as S};

require_once 'bootstrap.php';

class Widget
{
    use Loggable;

    public function run(SaverContract $saver): void
    {
        bootstrap();
        $this->step();
        self::build();
        static::make();
        $label = S::shout('w');
        $saver->save($this);
        $this->log($label);
    }

    public function step(): void
    {
        $this->ping();
    }

    public function ping(): void
    {
    }

    public static function build(): void
    {
    }

    public static function make(): void
    {
    }
}

class Gadget
{
    public function step(): void
    {
        $this->ping();
    }

    public function ping(): void
    {
    }
}
