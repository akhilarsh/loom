<?php
use App\Core\{Widget, DiskSaver};

function test_run(): void
{
    $w = new Widget();
    $w->run(new DiskSaver());
}
